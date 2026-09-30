//! Integration tests for the PR review "suggested change" local apply
//! (#351, ADR-0172 / ADR-0210).
//!
//! Verifies the full plan → confirm → preflight → execute → verify → oplog
//! path through `Backend::run` / `run_recorded`:
//! - applying a suggestion replaces EXACTLY the anchored range in the
//!   working-tree file, and the index (staged paths / OIDs / modes) is left
//!   exactly as it was;
//! - the working-tree file must be the PR head's blob at that path: a local
//!   edit, or a head that is not in the object store, refuses the apply;
//! - a change AFTER the plan was confirmed refuses at preflight — never edit
//!   the wrong lines;
//! - a successful apply is recorded in the oplog as `op="apply-suggestion"`
//!   with its backup ref, and the pre-apply bytes come back from that ref
//!   after a gc that pruned every other copy of them.
//!
//! All writes are confined to `TempDir` repositories. `KAGI_LOG_DIR` is
//! process-global, so oplog tests serialize on `ENV_LOCK`.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_command, git_output, init_repo};

use tempfile::TempDir;

use kagi_domain::plan_note::{GithubNote, PlanNote};
use kagi_git::oplog::{read_oplog_tail, OpOutcome, RecoveryHandle};
use kagi_git::{Backend, CommitId, Operation, OperationOutcome, Suggestion};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// A repository with `src/lib.rs` (3 lines) committed on `main`, HEAD
/// attached and clean, and an isolated oplog directory.
struct Fixture {
    _lock: MutexGuard<'static, ()>,
    repo: TempDir,
    _log: TempDir,
    previous: Option<std::ffi::OsString>,
}

impl Fixture {
    fn new() -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let repo = TempDir::new().unwrap();
        let log = TempDir::new().unwrap();
        let previous = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", log.path());
        init_repo(repo.path(), "main");
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("src/lib.rs"), "one\ntwo\nthree\n").unwrap();
        commit_all(repo.path(), "c1");
        Self {
            _lock: lock,
            repo,
            _log: log,
            previous,
        }
    }

    fn path(&self) -> &Path {
        self.repo.path()
    }

    fn backend(&self) -> Backend {
        let mut backend = Backend::open(self.path()).expect("open");
        // The backup ref must hold on its own, without a savepoint beside it.
        backend.set_auto_snapshot(false);
        backend
    }

    fn head(&self) -> CommitId {
        CommitId(
            git_output(self.path(), &["rev-parse", "HEAD"])
                .trim()
                .to_string(),
        )
    }

    fn file(&self) -> String {
        std::fs::read_to_string(self.path().join("src/lib.rs")).unwrap()
    }

    /// Capture the range and build the operation against `head`.
    fn op(&self, s: Suggestion, head: CommitId) -> Operation {
        let expected = self
            .backend()
            .capture_suggestion_context(&s)
            .expect("capture");
        Operation::ApplySuggestion {
            suggestion: s,
            expected_original: expected,
            head,
        }
    }

    /// Every index entry's (mode, OID, stage, path).
    fn index(&self) -> String {
        git_output(self.path(), &["ls-files", "--stage"])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var("KAGI_LOG_DIR", value),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}

fn suggestion(start: u32, end: u32, replacement: &str) -> Suggestion {
    Suggestion {
        path: "src/lib.rs".into(),
        start_line: start,
        end_line: end,
        replacement: replacement.into(),
    }
}

fn github_blockers(plan: &kagi_git::OperationPlan) -> Vec<GithubNote> {
    plan.blockers
        .iter()
        .filter_map(|note| match note {
            PlanNote::Github(note) => Some(note.clone()),
            _ => None,
        })
        .collect()
}

// ── applying replaces exactly the anchored range, index untouched ─────

#[test]
fn apply_replaces_exactly_the_anchored_range_and_keeps_the_index() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = Fixture::new();
    // Something already staged elsewhere: the apply must leave it alone.
    std::fs::write(f.path().join("staged.txt"), "staged\n").unwrap();
    git(f.path(), &["add", "staged.txt"]);
    let index_before = f.index();

    let op = f.op(suggestion(2, 2, "TWO"), f.head());
    let Operation::ApplySuggestion {
        expected_original, ..
    } = &op
    else {
        unreachable!()
    };
    assert_eq!(expected_original, &vec!["two".to_string()]);
    let mut backend = f.backend();
    let plan = backend.plan(&op).expect("plan");
    assert!(plan.blockers.is_empty(), "fresh suggestion has no blockers");
    assert!(plan.destructive, "a working-tree rewrite is destructive");
    assert_eq!(plan.equivalent_command, None, "git has no equivalent");
    backend.run(&op, &plan).expect("run");

    // Only line 2 changed; lines 1 and 3 untouched; trailing newline kept.
    assert_eq!(f.file(), "one\nTWO\nthree\n");
    assert_eq!(
        f.index(),
        index_before,
        "apply must not touch the index (working tree only)"
    );
}

// ── the working-tree file must be the PR head's blob ─────────────

#[test]
fn a_local_edit_to_the_file_refuses_the_apply() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = Fixture::new();
    // Same line count, same anchored line — only another line differs, so
    // only the head-blob comparison can tell.
    std::fs::write(f.path().join("src/lib.rs"), "ONE\ntwo\nthree\n").unwrap();
    let op = f.op(suggestion(2, 2, "TWO"), f.head());
    let mut backend = f.backend();
    let plan = backend.plan(&op).expect("plan");
    assert_eq!(
        github_blockers(&plan),
        vec![GithubNote::SuggestionNotPrHead {
            path: "src/lib.rs".into()
        }]
    );
    let err = backend
        .run(&op, &plan)
        .expect_err("a blocked plan must not run");
    assert!(err.to_string().contains("blocker"), "got: {err}");
    assert_eq!(f.file(), "ONE\ntwo\nthree\n", "refusal must not write");
}

#[test]
fn a_head_missing_from_the_object_store_refuses_the_apply() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = Fixture::new();
    let op = f.op(
        suggestion(2, 2, "TWO"),
        CommitId("1234567890abcdef1234567890abcdef12345678".into()),
    );
    let plan = f.backend().plan(&op).expect("plan");
    assert_eq!(
        github_blockers(&plan),
        vec![GithubNote::SuggestionHeadUnavailable]
    );
}

// ── a change after the confirm → REFUSES (TOCTOU guard) ──────────

#[test]
fn a_change_after_plan_makes_execute_refuse() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = Fixture::new();
    let op = f.op(suggestion(2, 2, "TWO"), f.head());
    let mut backend = f.backend();
    let plan = backend.plan(&op).expect("plan");
    assert!(plan.blockers.is_empty());

    // TOCTOU: the anchored line changes AFTER the plan was built and confirmed.
    std::fs::write(f.path().join("src/lib.rs"), "one\nEDITED\nthree\n").unwrap();

    let err = backend
        .run(&op, &plan)
        .expect_err("a changed file must refuse");
    // #502 refuses at the shared preflight, retaining the concrete blocker.
    assert!(matches!(&err, kagi_git::GitError::Preflight(_)));
    assert!(
        err.to_string().contains("src/lib.rs")
            && err.to_string().contains("not the pull request head"),
        "refusal names the file, got: {err}"
    );
    assert_eq!(f.file(), "one\nEDITED\nthree\n", "refusal must not write");
}

// ── oplog receipt + ref-backed recovery ───────────────────────────

#[test]
fn apply_is_recorded_and_recoverable_from_its_backup_ref_after_gc() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = Fixture::new();
    // The PR head is a commit no ref points at, and the working tree holds
    // its version of the file uncommitted: after a gc the backup ref is the
    // only thing keeping the pre-apply bytes.
    let original = "pr head line 1\npr head line 2\n";
    std::fs::write(f.path().join("src/lib.rs"), original).unwrap();
    git(f.path(), &["add", "src/lib.rs"]);
    let tree = git_output(f.path(), &["write-tree"]).trim().to_string();
    git(f.path(), &["reset", "-q"]);
    let head = git_output(f.path(), &["commit-tree", &tree, "-m", "pr head"])
        .trim()
        .to_string();
    let index_before = f.index();

    let op = f.op(suggestion(1, 2, "applied\n"), CommitId(head));
    let mut backend = f.backend();
    let plan = backend.plan(&op).expect("plan");
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let report = backend.run_recorded(&op, &plan);
    let Ok(OperationOutcome::Suggestion(outcome)) = &report.result else {
        panic!("apply failed: {:?}", report.result.as_ref().err())
    };
    assert_eq!(f.file(), "applied\n");
    assert_eq!(f.index(), index_before, "the index is untouched");

    let persisted = read_oplog_tail(10)
        .into_iter()
        .rev()
        .find(|e| e.op == "apply-suggestion")
        .expect("apply-suggestion recorded in oplog");
    assert!(
        matches!(persisted.outcome, OpOutcome::Success { .. }),
        "successful apply logs Success"
    );
    assert!(outcome.reference.starts_with("refs/kagi/backups/"));
    assert_eq!(persisted.backup_refs, vec![outcome.reference.clone()]);
    assert_eq!(
        persisted.recovery,
        vec![RecoveryHandle::file(
            "src/lib.rs",
            outcome.backup_blob.clone(),
            Some(outcome.reference.clone()),
        )]
    );

    git(f.path(), &["reflog", "expire", "--expire=now", "--all"]);
    git(f.path(), &["gc", "-q", "--prune=now"]);
    let name = &persisted.backup_refs[0];
    let bytes = f.backend().read_backup(name).expect("backup survives gc");
    assert_eq!(bytes, original.as_bytes());
    let cat = git_command(f.path())
        .args(["cat-file", "blob", name])
        .output()
        .unwrap();
    assert_eq!(cat.stdout, original.as_bytes());

    // Restoring is writing those bytes back.
    std::fs::write(f.path().join("src/lib.rs"), &bytes).unwrap();
    assert_eq!(f.file(), original);
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
