//! Repository-scoped conflict admission, using a real independent lock owner.
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod isolated;
use git_fixture::{commit_all, git, git_output, git_succeeds, init_repo, write_file};
use kagi_domain::plan_note::{ConflictsNote, PlanNote};
use kagi_git::backend::ExecutionPolicy;
use kagi_git::oplog::OpOutcome;
use kagi_git::{Backend, GitError, ResolutionChoice};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

#[test]
fn independent_lock_owner() {
    let Some(path) = std::env::var_os("KAGI_WRITE_LOCK_CHILD") else {
        return;
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap();
    file.try_lock().unwrap();
    println!("LOCKED");
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

struct Owner(Child);
impl Owner {
    fn start(repo: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "independent_lock_owner", "--nocapture"])
            .env("KAGI_WRITE_LOCK_CHILD", repo.join(".git/kagi-write.lock"))
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        loop {
            let mut line = String::new();
            assert_ne!(output.read_line(&mut line).unwrap(), 0, "lock child exited");
            if line.trim() == "LOCKED" {
                break;
            }
        }
        Self(child)
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn conflict(root: &Path) -> PathBuf {
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo, "main");
    write_file(&repo, "file.txt", "base\n");
    commit_all(&repo, "base");
    git(&repo, &["checkout", "-qb", "side"]);
    write_file(&repo, "file.txt", "side\n");
    commit_all(&repo, "side");
    git(&repo, &["checkout", "-q", "main"]);
    write_file(&repo, "file.txt", "main\n");
    commit_all(&repo, "main");
    assert!(!git_succeeds(&repo, &["merge", "side"]));
    repo
}

fn witness(repo: &Path) -> (String, Vec<u8>, Vec<u8>, Vec<u8>) {
    (
        git_output(repo, &["rev-parse", "HEAD"]),
        std::fs::read(repo.join(".git/index")).unwrap(),
        std::fs::read(repo.join("file.txt")).unwrap(),
        std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
    )
}
fn busy(error: &GitError) {
    assert_eq!(
        error.blocker(),
        Some(&PlanNote::Conflicts(ConflictsNote::RepositoryBusy))
    );
    assert_eq!(
        error.to_string(),
        "Another Kagi window is operating on this repository"
    );
}

#[test]
fn admission_probe_is_prompt_and_receipt_free_until_owner_death() {
    if !isolated::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo, "main");
    let receipts_before = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len();
    let owner = Owner::start(repo);
    // Repeated probes distinguish zero-timeout admission from eight bounded
    // 100ms waits, with headroom for repository discovery and scheduling.
    let started = std::time::Instant::now();
    for _ in 0..8 {
        busy(&Backend::check_conflict_write_admission(repo).unwrap_err());
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(400),
        "admission probes must not sleep for the planner/executor timeout: {elapsed:?}"
    );
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
        receipts_before,
        "busy probes must not write refusal receipts"
    );

    drop(owner); // SIGKILL + wait releases the independent owner's descriptor.
    Backend::check_conflict_write_admission(repo).unwrap();
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
        receipts_before,
        "successful probes must not write receipts either"
    );
    assert!(
        repo.join(".git/kagi-write.lock").exists(),
        "owner death must not unlink the shared lock sidecar"
    );
    // A successful probe releases admission rather than retaining ownership.
    let _next_owner = Owner::start(repo);
    busy(&Backend::check_conflict_write_admission(repo).unwrap_err());
}

#[test]
fn child_lock_refuses_save_continue_skip_abort_without_writes_and_death_releases_it() {
    if !isolated::run_isolated() {
        return;
    }
    for route in ["save", "continue", "abort"] {
        let tmp = tempfile::tempdir().unwrap();
        let repo = conflict(tmp.path());
        let b = Backend::open(&repo).unwrap();
        let snapshot = b.conflict_snapshot().unwrap().unwrap();
        let mut buffer = b.resolution_buffer_from_repo().unwrap();
        buffer
            .apply_choice(Path::new("file.txt"), ResolutionChoice::Current)
            .unwrap();
        let request = if route == "abort" {
            kagi_domain::conflict_family::ConflictRequest::Abort {
                revision: snapshot.observation.revision.clone(),
                kind: snapshot.observation.kind,
            }
        } else {
            Backend::conflict_save_request(
                snapshot.observation.revision.clone(),
                &buffer,
                Path::new("file.txt"),
                snapshot.observation.kind,
                "",
            )
            .unwrap()
        };
        let plan = Backend::plan_recorded_conflict(&repo, request.clone()).unwrap();
        let linked = tmp.path().join("linked");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-qb",
                "linked",
                linked.to_str().unwrap(),
                "HEAD",
            ],
        );
        let before = witness(&repo);
        let owner = Owner::start(&repo);
        // Reads stay available while another process owns write admission.
        assert!(b.conflict_snapshot().unwrap().is_some());
        let receipts_before = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).len();
        let linked_receipts_before = kagi_git::oplog::read_oplog_tail_for_repo(&linked, 100).len();
        busy(&Backend::check_conflict_write_admission(&linked).unwrap_err());
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).len(),
            receipts_before,
            "linked admission probe must leave receipt ownership to its caller"
        );
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(&linked, 100).len(),
            linked_receipts_before,
            "linked admission probe must not append a worktree-local receipt"
        );
        busy(&Backend::plan_recorded_conflict(&repo, request).unwrap_err());
        // The header's early Abort planning refusal has no recorded conflict
        // job yet: backend admission must persist it before returning to UI.
        busy(&b.plan_operation_abort().unwrap_err());
        let receipt = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 1)
            .pop()
            .unwrap();
        assert!(matches!(&receipt.outcome, OpOutcome::Refused { blockers }
            if blockers == &["Another Kagi window is operating on this repository"]));
        assert_eq!(receipt.ref_moves, Some(Vec::new()));
        if route == "continue" {
            // Planning returns the typed refusal; the caller owns its receipt.
            let receipts_before = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).len();
            busy(
                &b.plan_conflict_continue(
                    &snapshot.session,
                    &buffer,
                    &snapshot.observation.revision,
                )
                .unwrap_err(),
            );
            assert_eq!(
                kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).len(),
                receipts_before,
                "Continue planning must not append an admission receipt"
            );
            busy(
                &b.plan_conflict_continue_route(
                    &snapshot.session,
                    &buffer,
                    "main",
                    &snapshot.observation.revision,
                )
                .unwrap_err(),
            );
            assert_eq!(
                kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).len(),
                receipts_before,
                "ContinueRoute planning must not append an admission receipt"
            );
            // A merge cannot be skipped: busy admission must win before
            // operation validation, without creating a second refusal receipt.
            busy(
                &b.plan_conflict_skip(&snapshot.session, &snapshot.observation.revision)
                    .unwrap_err(),
            );
            assert_eq!(
                kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).len(),
                receipts_before,
                "Skip planning must not append an admission receipt"
            );
            busy(
                &b.execute_conflict_continue(
                    &snapshot.session,
                    &buffer,
                    &snapshot.observation.revision,
                )
                .unwrap_err(),
            );
        } else {
            let report = Backend::run_recorded_conflict(&plan, ExecutionPolicy::human(false), None);
            assert!(
                matches!(report.recording.entry().outcome, OpOutcome::Refused { .. }),
                "{route}: {:?}",
                report.recording
            );
            assert_eq!(
                report.blocker,
                Some(PlanNote::Conflicts(ConflictsNote::RepositoryBusy))
            );
        }
        assert_eq!(
            witness(&repo),
            before,
            "{route} wrote while another process held admission"
        );
        drop(owner); // SIGKILL + wait: kernel releases the fd lock, no unlink.
        if route == "continue" {
            // Nested continue → stage admission shares one descriptor, not a
            // second independently opened flock that would deadlock/refuse.
            b.execute_conflict_continue(&snapshot.session, &buffer, &snapshot.observation.revision)
                .unwrap();
        } else {
            let report = Backend::run_recorded_conflict(&plan, ExecutionPolicy::human(false), None);
            assert!(
                matches!(report.recording.entry().outcome, OpOutcome::Success { .. }),
                "{route}: {:?}",
                report.recording
            );
        }
        assert_ne!(
            witness_after(&repo),
            (before.0, before.1),
            "{route} did not execute after owner death"
        );
        assert!(
            repo.join(".git/kagi-write.lock").exists(),
            "persistent sidecar must not be unlinked"
        );
    }
}
fn witness_after(repo: &Path) -> (String, Vec<u8>) {
    (
        git_output(repo, &["rev-parse", "HEAD"]),
        std::fs::read(repo.join(".git/index")).unwrap(),
    )
}

#[test]
fn legacy_abort_and_merge_plans_return_one_operation_named_receipt() {
    if !isolated::run_isolated() {
        return;
    }
    for kind in ["merge", "cherry-pick"] {
        let tmp = tempfile::tempdir().unwrap();
        let repo = conflict(tmp.path());
        if kind == "cherry-pick" {
            git(&repo, &["merge", "--abort"]);
            assert!(!git_succeeds(&repo, &["cherry-pick", "side"]));
        }
        let backend = Backend::open(&repo).unwrap();
        let snapshot = backend.conflict_snapshot().unwrap().unwrap();
        let _owner = Owner::start(&repo);
        for route in ["session-abort", "operation-abort", "merge-commit"] {
            let before = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).len();
            let error = match route {
                "session-abort" => backend.plan_conflict_abort(&snapshot.session),
                "operation-abort" => backend.plan_operation_abort(),
                _ => backend.plan(&kagi_git::Operation::MergeCommit {
                    message: "merge".into(),
                }),
            }
            .unwrap_err();
            busy(&error);
            let expected = if route == "merge-commit" {
                route.to_string()
            } else {
                format!("{kind}-abort")
            };
            let recording = error.recording().expect("retain attempted receipt");
            assert!(matches!(
                recording,
                kagi_git::backend::recording::Recording::Appended { .. }
            ));
            let entries = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100);
            assert_eq!(entries.len(), before + 1, "exactly one refusal per attempt");
            assert_eq!(recording.entry().op, expected);
            assert_eq!(entries.first().unwrap(), recording.entry());
            assert!(matches!(
                recording.entry().outcome,
                OpOutcome::Refused { .. }
            ));
            assert_eq!(recording.entry().ref_moves, Some(Vec::new()));
        }
    }
}

#[test]
fn legacy_plan_refusals_preserve_failed_recording_through_error_wrappers() {
    if !isolated::run_isolated() {
        return;
    }
    // A directory at the append target fails even when tests run as root.
    let log = PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap());
    std::fs::create_dir(log.join("operations.jsonl")).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let repo = conflict(tmp.path());
    let backend = Backend::open(&repo).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let before = witness(&repo);
    let _owner = Owner::start(&repo);
    for route in ["session-abort", "operation-abort", "merge-commit"] {
        let error = match route {
            "session-abort" => backend.plan_conflict_abort(&snapshot.session),
            "operation-abort" => backend.plan_operation_abort(),
            _ => backend.plan(&kagi_git::Operation::MergeCommit {
                message: "merge".into(),
            }),
        }
        .unwrap_err();
        busy(&error);
        let recording = error.recording().expect("failed receipt must reach UI");
        let kagi_git::backend::recording::Recording::Failed {
            attempted,
            error: append_error,
        } = recording
        else {
            panic!("expected failed append: {recording:?}");
        };
        assert!(!append_error.is_empty());
        assert_eq!(
            attempted.op,
            if route == "merge-commit" {
                "merge-commit"
            } else {
                "merge-abort"
            }
        );
        assert_eq!(
            std::fs::canonicalize(&attempted.repo).unwrap(),
            std::fs::canonicalize(&repo).unwrap()
        );
        assert!(matches!(&attempted.outcome, OpOutcome::Refused { blockers }
            if blockers == &["Another Kagi window is operating on this repository"]));
        assert_eq!(attempted.ref_moves, Some(Vec::new()));
        let original = recording.clone();
        let wrapped = GitError::Preflight(Box::new(error));
        busy(&wrapped);
        assert_eq!(wrapped.recording(), Some(&original));
        assert!(kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).is_empty());
        assert_eq!(witness(&repo), before);
    }
}
