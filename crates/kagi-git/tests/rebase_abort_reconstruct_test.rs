//! #534: abort restores ORIG_HEAD, but reconstructs rebase output onto HEAD.
use git2::{Repository, RepositoryState};
use kagi_git::{
    detect_conflict_session, execute_conflict_abort, execute_conflict_continue, ConflictOp,
    ResolutionBuffer,
};
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Mutex, MutexGuard};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    dir: TempDir,
    old_log: Option<std::ffi::OsString>,
    original: git2::Oid,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        match &self.old_log {
            Some(value) => std::env::set_var("KAGI_LOG_DIR", value),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}

fn git_output(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_EDITOR", "true")
        .output()
        .expect("run fixture git")
}

fn git(dir: &Path, args: &[&str]) {
    let output = git_output(dir, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", message]);
}

impl Fixture {
    /// A zero-conflict step adds its own file and replays cleanly; other steps
    /// edit that many of the four conflict candidates.
    fn new(data_files: usize, changed_files: usize, steps: &[usize]) -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let old_log = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", dir.path().join("logs"));
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        git(&root, &["config", "rerere.enabled", "false"]);
        for i in 0..data_files {
            write(&root, &format!("data-{i:04}.txt"), "original\n");
        }
        for i in 0..4 {
            write(&root, &format!("conflict-{i}.txt"), "original\n");
        }
        commit(&root, "base");
        git(&root, &["switch", "-c", "topic"]);
        for (step, &count) in steps.iter().enumerate() {
            if count == 0 {
                write(&root, &format!("applied-{step}.txt"), "clean replay\n");
            }
            for i in 0..count {
                write(
                    &root,
                    &format!("conflict-{i}.txt"),
                    &format!("topic step {step}\n"),
                );
            }
            commit(&root, &format!("topic step {step}"));
        }
        let original = Repository::open(&root)
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap();
        git(&root, &["switch", "main"]);
        for i in 0..changed_files {
            write(&root, &format!("data-{i:04}.txt"), "upstream update\n");
        }
        for i in 0..4 {
            write(&root, &format!("conflict-{i}.txt"), "upstream\n");
        }
        commit(&root, "upstream");
        git(&root, &["switch", "topic"]);
        let rebase = git_output(&root, &["rebase", "--merge", "main"]);
        assert!(!rebase.status.success(), "fixture must stop at a conflict");
        Self {
            _guard: guard,
            dir,
            old_log,
            original,
        }
    }

    fn repo(&self) -> Repository {
        Repository::open(self.dir.path().join("repo")).unwrap()
    }

    fn assert_restored(&self) {
        let repo = self.repo();
        assert_eq!(repo.head().unwrap().name().unwrap(), "refs/heads/topic");
        assert_eq!(repo.head().unwrap().target(), Some(self.original));
        assert!(!repo.head_detached().unwrap());
        let original_tree = repo.find_commit(self.original).unwrap().tree_id();
        let mut index = repo.index().unwrap();
        assert!(!index.has_conflicts());
        assert_eq!(
            index.write_tree().unwrap(),
            original_tree,
            "entire index restored"
        );
        assert!(
            repo.statuses(None).unwrap().is_empty(),
            "entire working tree restored"
        );
        assert_eq!(repo.state(), RepositoryState::Clean);
        assert!(detect_conflict_session(&repo).is_none());
        for state in ["rebase-merge", "rebase-apply", "REBASE_HEAD"] {
            assert!(!repo.path().join(state).exists(), "leftover {state}");
        }
    }
}

#[test]
fn abort_large_rebase_restores_branch_head_index_worktree_and_state() {
    // 1,678 data files + conflict-3 are legitimate upstream-only changes.
    let fixture = Fixture::new(2000, 1678, &[3]);
    let repo = fixture.repo();
    let session = detect_conflict_session(&repo).unwrap();
    assert_eq!(session.files.len(), 3);
    let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    execute_conflict_abort(&repo, &session, &buffer).unwrap_or_else(|error| {
        panic!(
            "untouched rebase must abort: {}",
            error.to_string().chars().take(220).collect::<String>()
        )
    });
    fixture.assert_restored();
}

#[test]
fn abort_second_stop_after_multiple_replayed_commits_restores_original_topic() {
    let fixture = Fixture::new(12, 8, &[0, 3, 0, 4]);
    let repo = fixture.repo();
    let first = detect_conflict_session(&repo).unwrap();
    assert!(matches!(
        first.op,
        ConflictOp::Rebase {
            step: 2,
            total: 4,
            ..
        }
    ));
    let first_head = repo.head().unwrap().target().unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    for file in &first.files {
        buffer
            .set_manual_text(&file.path, "first stop resolution\n")
            .unwrap();
    }
    execute_conflict_continue(&repo, repo.workdir().unwrap(), &first, &buffer).unwrap();
    let repo = fixture.repo();
    let second = detect_conflict_session(&repo).unwrap();
    assert!(matches!(
        second.op,
        ConflictOp::Rebase {
            step: 4,
            total: 4,
            ..
        }
    ));
    assert_eq!(second.files.len(), 4);
    assert_ne!(repo.head().unwrap().target(), Some(first_head));
    assert!(repo.workdir().unwrap().join("applied-2.txt").exists());
    let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    execute_conflict_abort(&repo, &second, &buffer)
        .expect("abort after clean replay and resolution");
    fixture.assert_restored();
}

#[test]
fn continue_redetects_each_following_rebase_stop_without_snapshot_reload() {
    let fixture = Fixture::new(12, 8, &[3, 4, 3]);
    for expected in [(1, 3), (2, 4), (3, 3)] {
        let repo = fixture.repo();
        let session = detect_conflict_session(&repo).expect("fresh conflict detection");
        assert!(matches!(
            session.op,
            ConflictOp::Rebase { step, total: 3, .. } if step == expected.0
        ));
        assert_eq!(session.files.len(), expected.1);
        let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        for file in &session.files {
            buffer
                .set_manual_text(&file.path, "continue resolution\n")
                .unwrap();
        }
        execute_conflict_continue(&repo, repo.workdir().unwrap(), &session, &buffer)
            .expect("continue to next rebase stop");
    }
    assert!(
        detect_conflict_session(&fixture.repo()).is_none(),
        "fresh detection clears after the final real continue"
    );
}

#[test]
fn abort_refuses_only_real_staged_or_unstaged_nonconflict_edits() {
    for staged in [false, true] {
        let fixture = Fixture::new(12, 8, &[3]);
        let repo = fixture.repo();
        let session = detect_conflict_session(&repo).unwrap();
        let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        let path = "data-0002.txt";
        write(repo.workdir().unwrap(), path, "my manual edit\n");
        if staged {
            git(repo.workdir().unwrap(), &["add", path]);
        }
        let before_head = repo.head().unwrap().target();
        let before_index = std::fs::read(repo.path().join("index")).unwrap();
        let error = execute_conflict_abort(&repo, &session, &buffer)
            .unwrap_err()
            .to_string();
        assert_eq!(error, format!(
            "git error: abort refused: 1 file(s) were edited during conflict resolution and would be overwritten: {path}. Commit, stash or revert those edits first."
        ));
        assert_eq!(repo.head().unwrap().target(), before_head);
        assert_eq!(
            std::fs::read(repo.path().join("index")).unwrap(),
            before_index
        );
        assert_eq!(
            std::fs::read_to_string(repo.workdir().unwrap().join(path)).unwrap(),
            "my manual edit\n"
        );
        assert!(detect_conflict_session(&repo).is_some());
        if staged {
            let entry = repo.index().unwrap().get_path(Path::new(path), 0).unwrap();
            assert_eq!(
                repo.find_blob(entry.id).unwrap().content(),
                b"my manual edit\n"
            );
        }
    }
}
