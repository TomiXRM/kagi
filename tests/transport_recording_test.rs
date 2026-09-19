//! #501 — a transport mutation records at its own execution boundary.
//!
//! The regression these guard: the PR merge used to append the oplog only from
//! the UI completion, and the completion helper drops that whole callback when the
//! tab switched mid-op (`OpDisposition::DropStale`). A merge that really
//! happened on GitHub then left no record at all. These tests never run a UI
//! callback — reaching the assertions IS the stale-completion case.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

use kagi_domain::github::{PrFieldEdit, ReviewVerdict};
use kagi_git::github::{
    merge_pr, plan_pr_comment, plan_pr_edit, plan_pr_merge, plan_pr_review, pr_comment, pr_edit,
    pr_review, MergeMethod,
};
use kagi_git::oplog::{read_oplog_tail, Actor, OpOutcome};

/// PATH and KAGI_LOG_DIR are process-global; the tests in this binary share them.
static ENV_LOCK: Mutex<()> = Mutex::new(());

const HEAD_SHA: &str = "1111111111111111111111111111111111111111";

const PR_JSON: &str = r#"[{"number":501,"title":"transport recording",
  "headRefName":"feat/x","headRefOid":"1111111111111111111111111111111111111111",
  "baseRefName":"main","isDraft":false,"reviewDecision":"APPROVED",
  "mergeable":"MERGEABLE","statusCheckRollup":[],
  "url":"https://example.invalid/acme/widgets/pull/501","author":{"login":"a"},
  "reviewRequests":[],"body":""}]"#;

/// The `<host>/<owner>/<repo>` the PR itself carries. Every write below is
/// handed this instead of letting the transport ask `gh repo view` for it:
/// that lookup is a network round trip, and a PR that already knows where it
/// lives must not need GitHub to be reachable before it can be written to.
const BASE_REPO: &str = "example.invalid/acme/widgets";

struct Environment {
    path: Option<OsString>,
    log: Option<OsString>,
}

impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in [("PATH", &self.path), ("KAGI_LOG_DIR", &self.log)] {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

impl Environment {
    /// Put `bin` first on PATH and point the oplog at `logs`.
    fn install(bin: &Path, logs: &Path) -> Self {
        let restore = Environment {
            path: std::env::var_os("PATH"),
            log: std::env::var_os("KAGI_LOG_DIR"),
        };
        let mut paths = vec![bin.to_path_buf()];
        paths.extend(std::env::split_paths(
            &restore.path.clone().unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        std::env::set_var("KAGI_LOG_DIR", logs);
        restore
    }
}

/// A stand-in `gh` so the transport runs for real without touching GitHub.
fn fake_gh(bin: &Path, body: &str) {
    let gh = bin.join("gh");
    std::fs::write(&gh, body).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
}

/// `gh pr <subcommand> …` — dispatch on `$2` so one script answers both the
/// merge and the state re-read the boundary does after a non-zero exit. The
/// `--json` field is matched anywhere in the argv, not at a fixed position:
/// both commands also carry `-R <host/owner/repo>` (#701 review 4).
fn gh_script(merge: &str, view: &str) -> String {
    format!(
        "#!/bin/sh\ncase \"$2\" in\nmerge) {merge} ;;\n\
         view) case \"$*\" in *mergedAt*) {view} ;; *) exit 2 ;; esac ;;\nesac\n"
    )
}

const MERGE_OK: &str = "echo '✓ Merged pull request #501'";
const MERGE_FAILS: &str = "echo 'Head branch was modified' >&2; exit 1";
const VIEW_MERGED: &str = r#"echo '{"mergedAt":"2026-09-07T00:00:00Z"}'"#;
const VIEW_OPEN: &str = r#"echo '{"mergedAt":null}'"#;
const VIEW_UNREACHABLE: &str = "echo 'could not connect to github.com' >&2; exit 1";

fn merge_plan() -> kagi_git::OperationPlan {
    let pr = kagi_git::github::parse_pr_list(PR_JSON).unwrap().remove(0);
    plan_pr_merge(&pr, MergeMethod::Squash, false, "branch 'main'".into())
}

/// bin / logs / workdir under one tempdir, with PATH and KAGI_LOG_DIR pointed
/// at them. The guard restores the environment on drop.
fn fixture(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, Environment) {
    let (bin, logs, workdir) = (root.join("bin"), root.join("logs"), root.join("repo"));
    for dir in [&bin, &logs, &workdir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let restore = Environment::install(&bin, &logs);
    (bin, workdir, restore)
}

fn latest_outcome() -> OpOutcome {
    latest_outcome_of("pr-merge")
}

/// The single entry the log must hold, and the op it must be filed under.
fn latest_outcome_of(op: &str) -> OpOutcome {
    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 1, "exactly one entry per accepted attempt");
    assert_eq!(entries[0].op, op);
    entries[0].outcome.clone()
}

#[test]
fn pr_merge_records_before_returning_when_the_ui_completion_is_dropped() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, logs, workdir) = (
        root.path().join("bin"),
        root.path().join("logs"),
        root.path().join("repo"),
    );
    for dir in [&bin, &logs, &workdir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let _restore = Environment::install(&bin, &logs);
    let plan = merge_plan();

    fake_gh(&bin, &gh_script(MERGE_OK, VIEW_OPEN));
    let report = merge_pr(&workdir, 501, MergeMethod::Squash, false, HEAD_SHA, &plan);
    assert!(report.result.is_ok(), "fake gh should merge");
    assert!(matches!(
        report.recording,
        kagi_git::backend::recording::Recording::Appended { .. }
    ));

    // No UI callback ran: this is the dropped-completion path.
    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 1, "the merge must be recorded exactly once");
    let entry = &entries[0];
    assert_eq!(entry.op, "pr-merge");
    assert_eq!(entry.actor, Actor::Human);
    assert_eq!(
        entry.worktree.as_deref(),
        Some(workdir.display().to_string().as_str()),
        "the recording must name the worktree it ran in"
    );
    let OpOutcome::Success { after } = &entry.outcome else {
        panic!("expected a recorded success, got {:?}", entry.outcome);
    };
    assert!(
        after.dirty.contains(HEAD_SHA),
        "the head the merge was bound to is the recovery handle: {}",
        after.dirty
    );

    // A refused merge is an accepted attempt too — one more entry, not zero.
    // The re-read proves it did NOT merge, which is what makes it Failed.
    fake_gh(&bin, &gh_script(MERGE_FAILS, VIEW_OPEN));
    let report = merge_pr(&workdir, 501, MergeMethod::Squash, false, HEAD_SHA, &plan);
    assert!(report.result.is_err());
    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 2);
    let OpOutcome::Failed { error } = &entries[0].outcome else {
        panic!("expected a recorded failure, got {:?}", entries[0].outcome);
    };
    assert!(error.contains("Head branch was modified"), "{error}");
}

#[test]
fn a_failed_gh_whose_reread_says_merged_is_not_recorded_as_a_failure() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // The server holds the truth: `gh` can fail after GitHub already merged
    // (a broken response, a failing post-merge step). Recording that as Failed
    // invites a second merge attempt.
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir, _restore) = fixture(root.path());
    fake_gh(&bin, &gh_script(MERGE_FAILS, VIEW_MERGED));

    let report = merge_pr(
        &workdir,
        501,
        MergeMethod::Squash,
        false,
        HEAD_SHA,
        &merge_plan(),
    );
    // ADR-0196 Wave 3: the result follows the receipt, so a merge the server
    // confirms is `Ok` even though `gh` itself exited non-zero — the lease is
    // released and no reconcile entry is parked for a merge that is done.
    assert!(
        matches!(
            report.result,
            Ok(kagi_git::OperationOutcome::PrMerge {
                confirmed: true,
                ..
            })
        ),
        "a confirmed merge must not be handed back as a failure: {:?}",
        report.result
    );
    let OpOutcome::Success { after } = latest_outcome() else {
        panic!("a merged PR must not be recorded as failed");
    };
    assert!(after.dirty.contains(HEAD_SHA));
}

#[test]
fn a_merged_pr_whose_branch_deletion_is_unproven_is_partial() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // `--delete-branch` was requested and `gh` failed after the merge landed:
    // the merge is done, the deletion is not confirmed. Neither Success nor
    // Failed is honest.
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir, _restore) = fixture(root.path());
    fake_gh(&bin, &gh_script(MERGE_FAILS, VIEW_MERGED));

    let report = merge_pr(
        &workdir,
        501,
        MergeMethod::Squash,
        true,
        HEAD_SHA,
        &merge_plan(),
    );
    assert!(
        matches!(
            report.result,
            Ok(kagi_git::OperationOutcome::PrMerge {
                confirmed: false,
                ..
            })
        ),
        "merged but unfinished is `Ok` and unconfirmed, never a plain failure: {:?}",
        report.result
    );
    let OpOutcome::Partial { after, error } = latest_outcome() else {
        panic!("an unconfirmed branch deletion after a merge must be partial");
    };
    assert!(after.dirty.contains(HEAD_SHA));
    assert!(error.contains("branch deletion unconfirmed"), "{error}");
}

#[test]
fn a_failed_gh_that_cannot_be_re_read_is_unknown_not_failed() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // Disconnected between the merge and the re-read: neither confirmed nor
    // refuted. The existing Unknown contract says so and forbids a retry.
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir, _restore) = fixture(root.path());
    fake_gh(&bin, &gh_script(MERGE_FAILS, VIEW_UNREACHABLE));

    let report = merge_pr(
        &workdir,
        501,
        MergeMethod::Squash,
        false,
        HEAD_SHA,
        &merge_plan(),
    );
    // The lease-retaining terminal: `apply` keeps the scope reserved and parks
    // a reconcile entry only because the result says `TerminationUnknown`.
    assert!(
        matches!(
            report.result,
            Err(kagi_git::GitError::TerminationUnknown(_))
        ),
        "an unreadable merge must be handed back as unconfirmed: {:?}",
        report.result
    );
    let OpOutcome::Unknown { after, evidence } = latest_outcome() else {
        panic!("an unreadable PR state must be Unknown, never an assumed failure");
    };
    assert!(after.dirty.contains(HEAD_SHA));
    assert!(evidence.contains("do not retry"), "{evidence}");
}

#[test]
fn pr_merge_reports_recording_failure_without_hiding_the_merge() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, logs, workdir) = (
        root.path().join("bin"),
        root.path().join("logs"),
        root.path().join("repo"),
    );
    for dir in [&bin, &logs, &workdir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    // Deterministic append failure: the log file path is a directory (EISDIR).
    std::fs::create_dir_all(logs.join("operations.jsonl")).unwrap();
    let _restore = Environment::install(&bin, &logs);
    fake_gh(&bin, &gh_script(MERGE_OK, VIEW_OPEN));

    let report = merge_pr(
        &workdir,
        501,
        MergeMethod::Squash,
        false,
        HEAD_SHA,
        &merge_plan(),
    );
    assert!(
        report.result.is_ok(),
        "a failed append must not turn a completed merge into a failure"
    );
    let kagi_git::backend::recording::Recording::Failed { attempted, .. } = &report.recording
    else {
        panic!("expected a failed recording, got an appended one");
    };
    assert_eq!(attempted.op, "pr-merge");
    assert!(matches!(attempted.outcome, OpOutcome::Success { .. }));
}

// ── `gh pr comment` — the same boundary contract, without a server re-read ──

/// The comment body arrives on **stdin** (`--body-file -`), so the fake `gh`
/// must drain it: what it captures is the proof the text never rode in argv.
/// `$2` dispatch as above — `pr comment …` and the `repo view` that resolves
/// the `-R` identity are answered by one script.
fn gh_comment_script(comment: &str) -> String {
    format!(
        "#!/bin/sh\ncase \"$2\" in\ncomment) cat > ./posted-body.txt; {comment} ;;\n\
         view) case \"$*\" in *owner,name*) echo 'acme/widgets' ;; *) exit 2 ;; esac ;;\n\
         *) exit 2 ;;\nesac\n"
    )
}

const COMMENT_OK: &str = "echo 'https://example.invalid/acme/widgets/pull/501#issuecomment-99'";
const COMMENT_FAILS: &str = "echo 'GraphQL: Resource not accessible by integration' >&2; exit 1";
const COMMENT_BODY: &str = "LGTM, shipping it";

fn comment_plan() -> kagi_git::OperationPlan {
    let pr = kagi_git::github::parse_pr_list(PR_JSON).unwrap().remove(0);
    plan_pr_comment(&pr, COMMENT_BODY)
}

#[test]
fn pr_comment_records_before_returning_when_the_ui_completion_is_dropped() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir, _restore) = fixture(root.path());
    fake_gh(&bin, &gh_comment_script(COMMENT_OK));

    let report = pr_comment(&workdir, BASE_REPO, 501, COMMENT_BODY, &comment_plan());
    let Ok(kagi_git::OperationOutcome::PrComment { number, detail }) = &report.result else {
        panic!("expected a posted comment, got {:?}", report.result);
    };
    assert_eq!(*number, 501);
    assert!(
        detail.contains("issuecomment-99"),
        "the new comment's URL is the handle that identifies it: {detail}"
    );
    assert!(matches!(
        report.recording,
        kagi_git::backend::recording::Recording::Appended { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(workdir.join("posted-body.txt")).unwrap(),
        COMMENT_BODY,
        "the body must reach gh on stdin, whole and unquoted"
    );

    // No UI callback ran: this is the dropped-completion path.
    let entries = read_oplog_tail(10);
    assert_eq!(
        entries.len(),
        1,
        "the comment must be recorded exactly once"
    );
    let entry = &entries[0];
    assert_eq!(entry.op, "pr-comment");
    assert_eq!(entry.actor, Actor::Human);
    assert_eq!(
        entry.worktree.as_deref(),
        Some(workdir.display().to_string().as_str()),
        "the recording must name the worktree it ran in"
    );
    let OpOutcome::Success { after } = &entry.outcome else {
        panic!("expected a recorded success, got {:?}", entry.outcome);
    };
    assert!(
        after.dirty.contains("issuecomment-99"),
        "the receipt keeps the comment's URL: {}",
        after.dirty
    );
}

#[test]
fn a_refused_pr_comment_is_still_recorded_with_ghs_own_reason() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // A refused post is an accepted attempt too — one entry, not zero. This is
    // the whole reason the boundary records instead of the UI completion.
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir, _restore) = fixture(root.path());
    fake_gh(&bin, &gh_comment_script(COMMENT_FAILS));

    let report = pr_comment(&workdir, BASE_REPO, 501, COMMENT_BODY, &comment_plan());
    assert!(report.result.is_err(), "a refused post is not an Ok");
    let OpOutcome::Failed { error } = latest_outcome_of("pr-comment") else {
        panic!("a clean non-zero exit must be recorded as Failed");
    };
    assert!(
        error.contains("Resource not accessible by integration"),
        "gh's own reason must survive into the receipt: {error}"
    );
}

// ── `gh pr review` — the verdict half of the same boundary contract ──

/// `$2` dispatch as above. A review only reads stdin when `--body-file -` is
/// in the argv, so the script drains it only then: what it captures is the
/// proof the review text never rode in argv, and its *absence* for a
/// wordless approval is the proof kagi did not invent an empty body file.
fn gh_review_script(review: &str) -> String {
    format!(
        "#!/bin/sh\ncase \"$2\" in\n\
         review) case \"$*\" in *--body-file*) cat > ./review-body.txt ;; esac; {review} ;;\n\
         view) case \"$*\" in *owner,name*) echo 'acme/widgets' ;; *) exit 2 ;; esac ;;\n\
         *) exit 2 ;;\nesac\n"
    )
}

const REVIEW_OK: &str = "echo 'https://example.invalid/acme/widgets/pull/501#pullrequestreview-7'";
const REVIEW_BODY: &str = "line 3 leaks the handle; please close it";

fn review_plan(verdict: ReviewVerdict, body: &str) -> kagi_git::OperationPlan {
    let pr = kagi_git::github::parse_pr_list(PR_JSON).unwrap().remove(0);
    plan_pr_review(&pr, verdict, body)
}

#[test]
fn a_wordless_approval_records_its_verdict_without_sending_a_body() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir, _restore) = fixture(root.path());
    fake_gh(&bin, &gh_review_script(REVIEW_OK));

    let report = pr_review(
        &workdir,
        BASE_REPO,
        501,
        ReviewVerdict::Approve,
        "",
        &review_plan(ReviewVerdict::Approve, ""),
    );
    let Ok(kagi_git::OperationOutcome::PrReview {
        number,
        verdict,
        detail,
    }) = &report.result
    else {
        panic!("expected a submitted review, got {:?}", report.result);
    };
    assert_eq!(*number, 501);
    assert_eq!(verdict, "approve", "the receipt names which review it was");
    assert!(
        detail.contains("pullrequestreview-7"),
        "the review's URL is the handle that identifies it: {detail}"
    );
    assert!(
        !workdir.join("review-body.txt").exists(),
        "a wordless approval must not hand gh a body file"
    );

    // No UI callback ran: this is the dropped-completion path.
    let OpOutcome::Success { after } = latest_outcome_of("pr-review") else {
        panic!("expected a recorded success");
    };
    assert!(
        after.dirty.contains("approve") && after.dirty.contains("pullrequestreview-7"),
        "the receipt keeps the verdict and the review's URL: {}",
        after.dirty
    );
}

#[test]
fn a_request_changes_review_sends_its_body_on_stdin_and_is_recorded() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir, _restore) = fixture(root.path());
    fake_gh(&bin, &gh_review_script(REVIEW_OK));

    let report = pr_review(
        &workdir,
        BASE_REPO,
        501,
        ReviewVerdict::RequestChanges,
        REVIEW_BODY,
        &review_plan(ReviewVerdict::RequestChanges, REVIEW_BODY),
    );
    let Ok(kagi_git::OperationOutcome::PrReview { verdict, .. }) = &report.result else {
        panic!("expected a submitted review, got {:?}", report.result);
    };
    assert_eq!(verdict, "request-changes");
    assert!(matches!(
        report.recording,
        kagi_git::backend::recording::Recording::Appended { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(workdir.join("review-body.txt")).unwrap(),
        REVIEW_BODY,
        "the body must reach gh on stdin, whole and unquoted"
    );

    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 1, "the review is recorded exactly once");
    let entry = &entries[0];
    assert_eq!(entry.op, "pr-review");
    assert_eq!(entry.actor, Actor::Human);
    assert_eq!(
        entry.worktree.as_deref(),
        Some(workdir.display().to_string().as_str()),
        "the recording must name the worktree it ran in"
    );
    let OpOutcome::Success { after } = &entry.outcome else {
        panic!("expected a recorded success, got {:?}", entry.outcome);
    };
    assert!(
        after.dirty.contains("request-changes"),
        "the receipt names the verdict: {}",
        after.dirty
    );
}

// ── `gh pr edit` — the metadata half, and the identity it must not look up ──

/// A `gh` that **refuses** to be asked where the repository is.
///
/// `gh repo view` is a network round trip. Every write here is handed the
/// PR's own `<host>/<owner>/<repo>`, so reaching for that lookup is a bug,
/// not a fallback — and a silent one, because on a working network it would
/// simply succeed. So the stand-in fails loudly instead: exit 9 and a marker
/// file the test can find. `$1 $2` distinguishes `repo view` from `pr view`.
///
/// `pr edit` records its whole argv, one argument per line: that capture is
/// the proof each value got its own flag rather than a comma-joined list.
fn gh_strict_script() -> String {
    format!(
        "#!/bin/sh\ncase \"$1 $2\" in\n\
         'repo view') echo 'gh repo view must not be called' > ./repo-view-called.txt; exit 9 ;;\n\
         esac\ncase \"$2\" in\n\
         edit) printf '%s\\n' \"$@\" > ./edit-argv.txt; echo '{PR_URL}' ;;\n\
         comment) cat > ./posted-body.txt; {COMMENT_OK} ;;\n\
         *) exit 2 ;;\nesac\n"
    )
}

const PR_URL: &str = "https://example.invalid/acme/widgets/pull/501";

fn edit_plan(edit: &PrFieldEdit) -> kagi_git::OperationPlan {
    let pr = kagi_git::github::parse_pr_list(PR_JSON).unwrap().remove(0);
    plan_pr_edit(&pr, edit)
}

/// Two reviewers added and one label removed, in one call: the argv proves
/// one flag per value (a comma-joined list would split a label containing a
/// comma) and that `-R` names the repository the mutation lands in.
#[test]
fn pr_edit_sends_one_flag_per_value_and_records_its_receipt() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir, _restore) = fixture(root.path());
    fake_gh(&bin, &gh_strict_script());

    let edit = PrFieldEdit {
        add_reviewers: vec!["bob".into(), "carol".into()],
        remove_labels: vec!["wip".into()],
        ..Default::default()
    };
    let report = pr_edit(&workdir, BASE_REPO, 501, &edit, &edit_plan(&edit));
    let Ok(kagi_git::OperationOutcome::PrEdit { number, detail }) = &report.result else {
        panic!("expected an applied edit, got {:?}", report.result);
    };
    assert_eq!(*number, 501);
    assert!(detail.contains("/pull/501"), "gh's own words: {detail}");

    let argv: Vec<String> = std::fs::read_to_string(workdir.join("edit-argv.txt"))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        argv,
        vec![
            "pr",
            "edit",
            "-R",
            BASE_REPO,
            "501",
            "--add-reviewer",
            "bob",
            "--add-reviewer",
            "carol",
            "--remove-label",
            "wip",
        ],
        "every value gets its own flag, and -R names the repository"
    );
    assert!(
        !workdir.join("repo-view-called.txt").exists(),
        "the PR's own base repo was in hand; gh repo view must not be asked"
    );

    // No UI callback ran: this is the dropped-completion path.
    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 1, "the edit is recorded exactly once");
    let entry = &entries[0];
    assert_eq!(entry.op, "pr-edit");
    assert_eq!(entry.actor, Actor::Human);
    assert_eq!(
        entry.worktree.as_deref(),
        Some(workdir.display().to_string().as_str()),
        "the recording must name the worktree it ran in"
    );
    let OpOutcome::Success { after } = &entry.outcome else {
        panic!("expected a recorded success, got {:?}", entry.outcome);
    };
    assert!(
        after.dirty.contains("+2 reviewers, -1 label") && after.dirty.contains("/pull/501"),
        "the receipt summarises the change and keeps gh's handle: {}",
        after.dirty
    );
}

/// The reported bug: commenting resolved `-R` with `gh repo view`, a network
/// call, so an unreachable GitHub failed with "not a GitHub repo" even though
/// the PR in hand already carried its identity. The stand-in `gh` here treats
/// that lookup as a hard error, so the only way this passes is by not making
/// it.
#[test]
fn a_comment_never_asks_gh_where_the_repo_is_when_the_pr_already_knows() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir, _restore) = fixture(root.path());
    fake_gh(&bin, &gh_strict_script());

    let report = pr_comment(&workdir, BASE_REPO, 501, COMMENT_BODY, &comment_plan());
    assert!(
        matches!(
            &report.result,
            Ok(kagi_git::OperationOutcome::PrComment { number: 501, .. })
        ),
        "the comment must post without a repository lookup, got {:?}",
        report.result
    );
    assert!(
        !workdir.join("repo-view-called.txt").exists(),
        "gh repo view is a network round trip the PR's base_repo makes unnecessary"
    );
    assert_eq!(
        std::fs::read_to_string(workdir.join("posted-body.txt")).unwrap(),
        COMMENT_BODY
    );
    assert!(matches!(
        latest_outcome_of("pr-comment"),
        OpOutcome::Success { .. }
    ));
}

#[path = "support/isolated.rs"]
mod test_support;
