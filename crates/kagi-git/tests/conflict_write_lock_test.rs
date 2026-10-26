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

/// A fresh executable, not another thread or another descriptor in this
/// process: the oracle must exercise the OS sidecar lock independently.
#[test]
fn independent_lock_probe() {
    let Some(path) = std::env::var_os("KAGI_WRITE_LOCK_PROBE") else {
        return;
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    let acquired = match file.try_lock() {
        Ok(()) => true,
        Err(std::fs::TryLockError::WouldBlock) => false,
        Err(error) => panic!("unexpected lock probe error: {error}"),
    };
    assert_eq!(
        acquired,
        std::env::var("KAGI_WRITE_LOCK_EXPECT").unwrap() == "free"
    );
}

#[cfg(unix)]
fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ready() {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(unix)]
fn probe_sidecar(repo: &Path, free: bool) {
    let mut child = Owner(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "independent_lock_probe", "--nocapture"])
            .env("KAGI_WRITE_LOCK_PROBE", repo.join(".git/kagi-write.lock"))
            .env("KAGI_WRITE_LOCK_EXPECT", if free { "free" } else { "busy" })
            .spawn()
            .unwrap(),
    );
    wait_until("independent lock probe exits", || {
        child.0.try_wait().unwrap().is_some()
    });
    assert!(child.0.wait().unwrap().success(), "independent lock probe");
}

/// The fake Git leader exits only after its same-group descendant is ready.
/// Release and pipe closure are independent handshakes, not elapsed sleeps.
/// Linux adopts/reaps the orphan here rather than trusting container PID 1.
/// Darwin's OS reaps the orphan; every Child handle we own is waited.
#[cfg(unix)]
struct LingeringGit {
    root: PathBuf,
    #[cfg(target_os = "linux")]
    descendant_reaped: std::cell::Cell<bool>,
}

#[cfg(unix)]
impl LingeringGit {
    fn install(root: &Path, route: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;
        // Isolated test process only. Linux containers need not have a PID 1
        // that reaps orphans, which would prevent an empty-group stop proof.
        #[cfg(target_os = "linux")]
        assert_eq!(unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) }, 0);
        let fixture = Self {
            root: root.join("lingering-git"),
            #[cfg(target_os = "linux")]
            descendant_reaped: std::cell::Cell::new(false),
        };
        std::fs::create_dir(&fixture.root).unwrap();
        let real_git = Command::new("/bin/sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        assert!(real_git.status.success());
        let real_git = String::from_utf8(real_git.stdout).unwrap();
        // All shell paths travel as quoted environment values, including the
        // real Git fallback used by preflight/read commands.
        std::env::set_var("KAGI_LOCK_FIXTURE", &fixture.root);
        std::env::set_var("KAGI_LOCK_REAL_GIT", real_git.trim());
        std::env::set_var("KAGI_LOCK_ROUTE", format!("--{route}"));
        std::fs::write(
            fixture.root.join("descendant"),
            r#"#!/bin/sh
echo "$$" > "$KAGI_LOCK_FIXTURE/descendant-pid"
: > "$KAGI_LOCK_FIXTURE/ready"
# Bounded even if the isolated harness is killed before its Drop cleanup.
n=0
while [ ! -f "$KAGI_LOCK_FIXTURE/release" ] && [ "$n" -lt 1500 ]; do
    if [ -f "$KAGI_LOCK_FIXTURE/close-pipes" ]; then
        exec </dev/null >/dev/null 2>&1
        : > "$KAGI_LOCK_FIXTURE/pipes-closed"
    fi
    sleep 0.02
    n=$((n + 1))
done
"#,
        )
        .unwrap();
        let git = fixture.root.join("git");
        std::fs::write(
            &git,
            r#"#!/bin/sh
case "$*" in
  *"cherry-pick $KAGI_LOCK_ROUTE")
    echo "$$" > "$KAGI_LOCK_FIXTURE/group"
    /bin/sh "$KAGI_LOCK_FIXTURE/descendant" &
    n=0
    while [ ! -f "$KAGI_LOCK_FIXTURE/ready" ] && [ "$n" -lt 500 ]; do
        sleep 0.02
        n=$((n + 1))
    done
    test -f "$KAGI_LOCK_FIXTURE/ready" || exit 91
    exit 0
    ;;
  *) exec "$KAGI_LOCK_REAL_GIT" "$@" ;;
esac
"#,
        )
        .unwrap();
        std::fs::set_permissions(git, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut paths = vec![fixture.root.clone()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        fixture
    }

    fn install_post_commit(root: &Path, repo: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        #[cfg(target_os = "linux")]
        assert_eq!(unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) }, 0);
        let fixture = Self {
            root: root.join("successful-hook"),
            #[cfg(target_os = "linux")]
            descendant_reaped: std::cell::Cell::new(false),
        };
        std::fs::create_dir(&fixture.root).unwrap();
        std::env::set_var("KAGI_LOCK_FIXTURE", &fixture.root);
        std::fs::write(
            fixture.root.join("descendant"),
            r#"#!/bin/sh
echo "$$" > "$KAGI_LOCK_FIXTURE/descendant-pid"
: > "$KAGI_LOCK_FIXTURE/ready"
# Bounded even if the isolated harness is killed before its Drop cleanup.
n=0
while [ ! -f "$KAGI_LOCK_FIXTURE/release" ] && [ "$n" -lt 1500 ]; do
    sleep 0.02
    n=$((n + 1))
done
"#,
        )
        .unwrap();
        let hook = repo.join(".git/hooks/post-commit");
        std::fs::write(
            &hook,
            r#"#!/bin/sh
# Git is the hook's parent and the process-group leader.
echo "$PPID" > "$KAGI_LOCK_FIXTURE/group"
# Close all inherited stdio before the helper starts: collectors must see EOF.
/bin/sh "$KAGI_LOCK_FIXTURE/descendant" </dev/null >/dev/null 2>&1 &
n=0
while [ ! -f "$KAGI_LOCK_FIXTURE/ready" ] && [ "$n" -lt 500 ]; do
    sleep 0.02
    n=$((n + 1))
done
test -f "$KAGI_LOCK_FIXTURE/ready" || exit 91
exit 0
"#,
        )
        .unwrap();
        std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700)).unwrap();
        fixture
    }

    fn signal(&self, name: &str) {
        std::fs::write(self.root.join(name), "").unwrap();
    }

    fn group(&self) -> u32 {
        std::fs::read_to_string(self.root.join("group"))
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    fn reap_descendant(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            if self.descendant_reaped.get() {
                return true;
            }
            let Some(pid) = std::fs::read_to_string(self.root.join("descendant-pid"))
                .ok()
                .and_then(|pid| pid.trim().parse::<i32>().ok())
            else {
                return false;
            };
            let mut status = 0;
            // Exact adopted child only, never waitpid(-1): Git and the
            // independent lock probes have separate owners.
            if unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } == pid {
                self.descendant_reaped.set(true);
            }
            self.descendant_reaped.get()
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    }
}

#[cfg(unix)]
impl Drop for LingeringGit {
    fn drop(&mut self) {
        let _ = std::fs::write(self.root.join("release"), "");
        // No panic in cleanup, including a regression assertion's unwind.
        if let Ok(group) = std::fs::read_to_string(self.root.join("group")) {
            if let Ok(group) = group.trim().parse::<u32>() {
                // Allow the shell's bounded self-failsafe as well as release.
                // Never signal a cached group ID: it may already be recycled.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
                loop {
                    let reaped = self.reap_descendant();
                    if (reaped && !kagi_git::proc::group_alive(group))
                        || std::time::Instant::now() >= deadline
                    {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
    }
}

#[cfg(unix)]
fn unaccounted_conflict_keeps_admission(route: &str) {
    if !isolated::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = conflict(tmp.path());
    git(&repo, &["merge", "--abort"]);
    assert!(!git_succeeds(&repo, &["cherry-pick", "side"]));
    let backend = Backend::open(&repo).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    let skip = backend
        .plan_conflict_skip(&snapshot.session, &snapshot.observation.revision)
        .unwrap();
    if route == "continue" {
        buffer
            .apply_choice(Path::new("file.txt"), ResolutionChoice::Current)
            .unwrap();
    }
    let fixture = LingeringGit::install(tmp.path(), route);
    let error = if route == "continue" {
        backend
            .execute_conflict_continue(&snapshot.session, &buffer, &snapshot.observation.revision)
            .unwrap_err()
    } else {
        backend.execute_conflict_skip(&skip, &buffer).unwrap_err()
    };
    let GitError::TerminationUnknown(kagi_git::Termination::Unaccounted { group, .. }) = error
    else {
        panic!("expected a real live-descendant termination: {error:?}");
    };
    assert_eq!(group, fixture.group());
    assert!(fixture.root.join("ready").exists());
    let descendant: i32 = std::fs::read_to_string(fixture.root.join("descendant-pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(unsafe { libc::getpgid(descendant) }, group as i32);
    assert!(kagi_git::proc::group_alive(group));
    probe_sidecar(&repo, false);
    // Intentionally the same thread that ran Backend: a retained weak
    // reentrancy entry must not grant fresh admission to quarantined ownership.
    busy(&Backend::check_conflict_write_admission(&repo).unwrap_err());

    fixture.signal("close-pipes");
    wait_until("descendant closes inherited pipes", || {
        fixture.root.join("pipes-closed").exists()
    });
    // Allow the collector/janitor to observe EOF. Group lifetime, not pipe
    // lifetime, is the stop proof; the descendant still awaits explicit release.
    let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while std::time::Instant::now() < until {
        assert!(kagi_git::proc::group_alive(group));
        busy(&Backend::check_conflict_write_admission(&repo).unwrap_err());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    probe_sidecar(&repo, false);

    fixture.signal("release");
    wait_until("adopted descendant is reaped", || fixture.reap_descendant());
    wait_until("descendant group stops", || {
        !kagi_git::proc::group_alive(group)
    });
    // No Sessions reconciliation: the runner's janitor releases admission.
    wait_until("janitor releases repository admission", || {
        Backend::check_conflict_write_admission(&repo).is_ok()
    });
    probe_sidecar(&repo, true);
}

#[cfg(unix)]
#[test]
fn continue_unaccounted_descendant_keeps_repository_admission_until_group_stops() {
    unaccounted_conflict_keeps_admission("continue");
}

#[cfg(unix)]
#[test]
fn skip_unaccounted_descendant_keeps_repository_admission_until_group_stops() {
    unaccounted_conflict_keeps_admission("skip");
}

#[cfg(unix)]
#[test]
fn successful_post_commit_background_helper_releases_repository_admission() {
    if !isolated::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo, "main");
    git(&repo, &["config", "commit.gpgsign", "false"]);
    git(&repo, &["config", "core.hooksPath", ".git/hooks"]);
    write_file(&repo, "file.txt", "base\n");
    commit_all(&repo, "base");
    let before = git_output(&repo, &["rev-parse", "HEAD"]);
    write_file(&repo, "file.txt", "first\n");
    git(&repo, &["add", "file.txt"]);
    let fixture = LingeringGit::install_post_commit(tmp.path(), &repo);
    let mut backend = Backend::open(&repo).unwrap();
    let op = kagi_git::Operation::Commit {
        message: "successful background hook".into(),
    };
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let report = backend.run_recorded(&op, &plan);
    assert!(report.result.is_ok(), "{:?}", report.result);
    assert!(matches!(
        &report.recording.entry().outcome,
        OpOutcome::Success { .. }
    ));
    let first = git_output(&repo, &["rev-parse", "HEAD"]);
    assert_ne!(first, before);
    let descendant: i32 = std::fs::read_to_string(fixture.root.join("descendant-pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let group = fixture.group();
    assert_eq!(unsafe { libc::getpgid(descendant) }, group as i32);
    assert!(kagi_git::proc::group_alive(group));
    // Probe outside this process as well as on the executor's own thread:
    // lexical reentrancy must not hide a quarantined OS lock after Success.
    probe_sidecar(&repo, true);
    Backend::check_conflict_write_admission(&repo).unwrap();

    // Keep the original helper alive, but do not create a second helper or
    // overwrite the exact PID that the fixture owns and reaps.
    std::fs::remove_file(repo.join(".git/hooks/post-commit")).unwrap();
    write_file(&repo, "file.txt", "second\n");
    git(&repo, &["add", "file.txt"]);
    let next = kagi_git::Operation::Commit {
        message: "next admitted commit".into(),
    };
    let plan = backend.plan(&next).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let next_report = backend.run_recorded(&next, &plan);
    assert!(next_report.result.is_ok(), "{:?}", next_report.result);
    assert!(matches!(
        &next_report.recording.entry().outcome,
        OpOutcome::Success { .. }
    ));
    assert_ne!(git_output(&repo, &["rev-parse", "HEAD"]), first);
    assert!(kagi_git::proc::group_alive(group));
    probe_sidecar(&repo, true);
    let receipts = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100);
    for report in [&report, &next_report] {
        let receipt = receipts
            .iter()
            .find(|entry| entry.id == report.recording.entry().id)
            .expect("successful commit has its own durable receipt");
        assert!(matches!(&receipt.outcome, OpOutcome::Success { .. }));
    }

    fixture.signal("release");
    wait_until("successful hook descendant is reaped", || {
        fixture.reap_descendant()
    });
    wait_until("successful hook group stops", || {
        !kagi_git::proc::group_alive(group)
    });
}
