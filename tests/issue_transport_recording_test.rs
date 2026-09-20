//! Issue writes record without a UI callback and never carry body text in argv.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use kagi_domain::plan_note::{GithubNote, PlanDisposition, PlanNote};
use kagi_git::backend::recording::{Recording, RunReport};
use kagi_git::github::{
    issue_comment, issue_comment_args, issue_create, issue_create_args, plan_issue_comment,
    plan_issue_create,
};
use kagi_git::oplog::{read_oplog_tail, Actor, OpOutcome};

const REPO: &str = "example.invalid/acme/widgets";
const TITLE: &str = "A title with 'quotes' and --flags";
const BODY: &str = "arbitrary body '$HOME'\n```rust\nfn main() {}\n```\n";
const URL: &str = "https://example.invalid/acme/widgets/issues/42";

/// Every test runs alone in an isolated child; process-global PATH and log
/// configuration never escape to the parent harness or race sibling tests.
struct Fixture {
    _root: tempfile::TempDir,
    workdir: PathBuf,
    logs: PathBuf,
}

impl Fixture {
    fn new(action: &str, read_body: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        let logs = root.path().join("logs");
        let workdir = root.path().join("repo");
        for dir in [&bin, &logs, &workdir] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let gh = bin.join("gh");
        let stdin = if read_body { "cat > ./body.txt" } else { ":" };
        std::fs::write(
            &gh,
            format!(
                "#!/bin/sh\ncase \"$1 $2\" in\n\
                 'repo view') echo unexpected > ./repo-view.txt; exit 9 ;;\n\
                 'issue create'|'issue comment')\n\
                 printf '%s\\n' \"$@\" > ./argv.txt\n\
                 echo attempt >> ./attempts.txt\n\
                 {stdin}\n{action}\n;;\n*) exit 8 ;;\nesac\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(gh, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        std::env::set_var("KAGI_LOG_DIR", &logs);
        Self {
            _root: root,
            workdir,
            logs,
        }
    }

    fn run(&self, create: bool, body: &str) -> RunReport {
        if create {
            issue_create(
                &self.workdir,
                REPO,
                TITLE,
                body,
                &plan_issue_create(REPO, TITLE, body),
            )
        } else {
            issue_comment(
                &self.workdir,
                REPO,
                42,
                body,
                &plan_issue_comment(42, TITLE, body),
            )
        }
    }

    fn assert_addressed_body(&self, create: bool) {
        assert_eq!(
            std::fs::read_to_string(self.workdir.join("body.txt")).unwrap(),
            BODY
        );
        let argv: Vec<String> = std::fs::read_to_string(self.workdir.join("argv.txt"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        let expected = if create {
            issue_create_args(REPO, TITLE)
        } else {
            issue_comment_args(REPO, 42)
        };
        assert_eq!(argv, expected);
        assert!(!argv.iter().any(|arg| arg.contains("arbitrary body")));
        assert!(!self.workdir.join("repo-view.txt").exists());
    }
}

fn op(create: bool) -> &'static str {
    if create {
        "issue-create"
    } else {
        "issue-comment"
    }
}

fn assert_receipt(report: &RunReport, create: bool, workdir: &Path) {
    assert!(matches!(report.recording, Recording::Appended { .. }));
    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 1, "one receipt without any UI callback");
    let entry = &entries[0];
    assert_eq!(entry.op, op(create));
    assert_eq!(entry.actor, Actor::Human);
    assert_eq!(entry.worktree.as_deref(), Some(workdir.to_str().unwrap()));
    let OpOutcome::Success { after } = &entry.outcome else {
        panic!("expected success: {:?}", entry.outcome);
    };
    assert!(after.dirty.contains(URL));
}

#[test]
fn issue_create_records_url_and_body_before_any_ui_completion() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new(&format!("echo '{URL}'"), true);
    let report = fixture.run(true, BODY);
    let Ok(kagi_git::OperationOutcome::IssueCreate { detail }) = &report.result else {
        panic!("{:?}", report.result);
    };
    assert_eq!(detail, URL);
    fixture.assert_addressed_body(true);
    assert_receipt(&report, true, &fixture.workdir);
}

#[test]
fn issue_comment_records_url_and_body_before_any_ui_completion() {
    if !test_support::run_isolated() {
        return;
    }
    let url = format!("{URL}#issuecomment-9");
    let fixture = Fixture::new(&format!("echo '{url}'"), true);
    let report = fixture.run(false, BODY);
    let Ok(kagi_git::OperationOutcome::IssueComment { number, detail }) = &report.result else {
        panic!("{:?}", report.result);
    };
    assert_eq!(*number, 42);
    assert_eq!(detail, &url);
    fixture.assert_addressed_body(false);
    assert_receipt(&report, false, &fixture.workdir);
}

#[test]
fn clean_issue_refusals_record_ghs_reason() {
    if !test_support::run_isolated() {
        return;
    }
    for create in [true, false] {
        let fixture = Fixture::new("echo 'permission denied' >&2; exit 1", true);
        let report = fixture.run(create, BODY);
        assert!(matches!(report.result, Err(kagi_git::GitError::Other(_))));
        let entries = read_oplog_tail(10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].op, op(create));
        let OpOutcome::Failed { error } = &entries[0].outcome else {
            panic!("{:?}", entries[0].outcome);
        };
        assert!(error.contains("permission denied"));
    }
}

#[test]
fn recording_failure_does_not_turn_a_posted_issue_into_a_retry() {
    if !test_support::run_isolated() {
        return;
    }
    for create in [true, false] {
        let fixture = Fixture::new(&format!("echo '{URL}'"), true);
        std::fs::create_dir(fixture.logs.join("operations.jsonl")).unwrap();
        let report = fixture.run(create, BODY);
        assert!(report.result.is_ok());
        let Recording::Failed { attempted, .. } = &report.recording else {
            panic!("expected recording failure");
        };
        assert_eq!(attempted.op, op(create));
        assert!(matches!(attempted.outcome, OpOutcome::Success { .. }));
    }
}

#[test]
fn incomplete_stdin_is_unknown_even_when_gh_exits_zero() {
    if !test_support::run_isolated() {
        return;
    }
    // Larger than any pipe capacity: refusing to read guarantees incomplete IO.
    let body = "x".repeat(4 * 1024 * 1024);
    for create in [true, false] {
        let fixture = Fixture::new("exit 0", false);
        assert_unknown(&fixture, create, &body);
    }
}

#[test]
fn a_signal_exit_is_not_a_clean_refusal() {
    if !test_support::run_isolated() {
        return;
    }
    for create in [true, false] {
        let fixture = Fixture::new("kill -TERM $$", true);
        assert_unknown(&fixture, create, BODY);
    }
}

fn assert_unknown(fixture: &Fixture, create: bool, body: &str) {
    let report = fixture.run(create, body);
    assert!(matches!(
        report.result,
        Err(kagi_git::GitError::TerminationUnknown(_))
    ));
    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].op, op(create));
    let OpOutcome::Unknown { evidence, .. } = &entries[0].outcome else {
        panic!("{:?}", entries[0].outcome);
    };
    assert!(evidence.contains("do not retry blindly"));
    assert_eq!(
        std::fs::read_to_string(fixture.workdir.join("attempts.txt")).unwrap(),
        "attempt\n"
    );
}

#[test]
fn issue_arguments_pin_the_repository_and_use_stdin() {
    assert_eq!(
        issue_create_args(REPO, TITLE),
        vec![
            "issue",
            "create",
            "-R",
            REPO,
            "--title",
            TITLE,
            "--body-file",
            "-"
        ]
    );
    assert_eq!(
        issue_comment_args(REPO, 42),
        vec!["issue", "comment", "-R", REPO, "42", "--body-file", "-"]
    );
}

#[test]
fn empty_issue_and_reply_bodies_are_plan_blockers() {
    for body in ["", " \n\t"] {
        for plan in [
            plan_issue_create(REPO, TITLE, body),
            plan_issue_comment(42, TITLE, body),
        ] {
            assert_eq!(plan.disposition, PlanDisposition::Blocked);
            assert!(!plan.blockers.is_empty());
        }
    }
    for plan in [
        plan_issue_create(REPO, TITLE, BODY),
        plan_issue_comment(42, TITLE, BODY),
    ] {
        assert_eq!(plan.disposition, PlanDisposition::Ready);
        assert!(!plan.destructive);
        assert!(plan.recovery.is_none());
    }
}

#[test]
fn empty_issue_title_is_a_create_only_plan_blocker() {
    for title in ["", " \n\t"] {
        let plan = plan_issue_create(REPO, title, BODY);
        assert_eq!(plan.disposition, PlanDisposition::Blocked);
        assert!(plan
            .blockers
            .contains(&PlanNote::Github(GithubNote::IssueTitleEmpty)));
    }

    let reply = plan_issue_comment(42, "", BODY);
    assert_eq!(reply.disposition, PlanDisposition::Ready);
    assert!(!reply
        .blockers
        .contains(&PlanNote::Github(GithubNote::IssueTitleEmpty)));
}

#[test]
fn issue_write_never_resolves_a_missing_repository_during_dispatch() {
    if !test_support::run_isolated() {
        return;
    }
    for create in [true, false] {
        let fixture = Fixture::new(&format!("echo '{URL}'"), true);
        let plan = if create {
            plan_issue_create("", TITLE, BODY)
        } else {
            plan_issue_comment(42, TITLE, BODY)
        };
        let report = if create {
            issue_create(&fixture.workdir, "", TITLE, BODY, &plan)
        } else {
            issue_comment(&fixture.workdir, "", 42, BODY, &plan)
        };
        let Err(kagi_git::GitError::Other(error)) = &report.result else {
            panic!(
                "missing frozen repository must be refused: {:?}",
                report.result
            );
        };
        assert!(error.contains("no frozen GitHub repository identity"));
        assert!(!fixture.workdir.join("repo-view.txt").exists());
        assert!(!fixture.workdir.join("attempts.txt").exists());

        let entries = read_oplog_tail(10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].op, op(create));
        assert!(matches!(entries[0].outcome, OpOutcome::Failed { .. }));
    }
}

#[path = "support/isolated.rs"]
mod test_support;
