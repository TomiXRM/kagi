//! Hunk-level stage / unstage (#842, Refs #357).
//!
//! A committed 20-line file edited at line 2 and line 18 diffs as two hunks.
//! Staging one puts exactly that edit in the index and leaves the other
//! unstaged; unstaging a staged hunk takes exactly that edit back out. A hunk
//! drawn before the file moved is refused with `HunkChanged` and nothing is
//! written. The working tree is never touched.

use std::path::Path;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git_command, git_output, init_repo};

use kagi_domain::diff::HunkRange;
use kagi_domain::plan_note::{CommonNote, PlanNote};
use kagi_git::{Backend, GitError};
use tempfile::TempDir;

const FILE: &str = "f.txt";

fn lines(edit: impl Fn(usize) -> Option<&'static str>) -> String {
    (1..=20)
        .map(|n| match edit(n) {
            Some(text) => format!("{text}\n"),
            None => format!("line {n}\n"),
        })
        .collect()
}

fn original() -> String {
    lines(|_| None)
}

/// Line 2 and line 18 edited: two hunks, far enough apart not to merge.
fn edited() -> String {
    lines(|n| match n {
        2 => Some("TWO"),
        18 => Some("EIGHTEEN"),
        _ => None,
    })
}

struct Repo {
    dir: TempDir,
}

impl Repo {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path(), "main");
        std::fs::write(dir.path().join(FILE), original()).unwrap();
        commit_all(dir.path(), "c1");
        std::fs::write(dir.path().join(FILE), edited()).unwrap();
        Self { dir }
    }
    fn path(&self) -> &Path {
        self.dir.path()
    }
    fn backend(&self) -> Backend {
        Backend::open(self.path()).expect("open")
    }
    fn unstaged(&self) -> Vec<HunkRange> {
        let diff = self.backend().unstaged_file_diff(Path::new(FILE)).unwrap();
        diff.hunks.iter().map(|h| h.range()).collect()
    }
    fn staged(&self) -> Vec<HunkRange> {
        let diff = self.backend().staged_file_diff(Path::new(FILE)).unwrap();
        diff.hunks.iter().map(|h| h.range()).collect()
    }
    /// The staged content of the file, byte for byte.
    fn index_text(&self) -> String {
        show_index(self.path(), FILE)
    }
    fn worktree_text(&self) -> String {
        std::fs::read_to_string(self.path().join(FILE)).unwrap()
    }
}

fn is_hunk_changed(err: &GitError) -> bool {
    matches!(err, GitError::Blocked(note)
        if matches!(**note, PlanNote::Common(CommonNote::HunkChanged { .. })))
}

fn show_index(repo: &Path, file: &str) -> String {
    let out = git_command(repo)
        .args(["show", &format!(":{file}")])
        .output()
        .unwrap();
    assert!(out.status.success(), "{file} is not in the index");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn staging_one_hunk_stages_exactly_that_edit() {
    let repo = Repo::new();
    let hunks = repo.unstaged();
    assert_eq!(hunks.len(), 2, "two separate edits diff as two hunks");

    repo.backend()
        .stage_hunk(Path::new(FILE), hunks[0])
        .expect("stage the first hunk");

    assert_eq!(
        repo.index_text(),
        lines(|n| (n == 2).then_some("TWO")),
        "the index holds the line-2 edit only"
    );
    assert_eq!(repo.staged().len(), 1, "one hunk staged");
    assert_eq!(
        repo.unstaged(),
        vec![hunks[1]],
        "the other hunk stays unstaged"
    );
    assert_eq!(
        repo.worktree_text(),
        edited(),
        "the working tree is untouched"
    );
}

#[test]
fn unstaging_one_hunk_takes_exactly_that_edit_out() {
    let repo = Repo::new();
    let hunks = repo.unstaged();
    let backend = repo.backend();
    backend.stage_hunk(Path::new(FILE), hunks[0]).unwrap();
    // After the first, the second hunk's numbers are unchanged (same lines).
    backend
        .stage_hunk(Path::new(FILE), repo.unstaged()[0])
        .unwrap();
    assert_eq!(repo.index_text(), edited(), "both edits staged");
    let staged = repo.staged();
    assert_eq!(staged.len(), 2);

    repo.backend()
        .unstage_hunk(Path::new(FILE), staged[1])
        .expect("unstage the second hunk");

    assert_eq!(
        repo.index_text(),
        lines(|n| (n == 2).then_some("TWO")),
        "only the line-18 edit left the index"
    );
    assert_eq!(repo.staged(), vec![staged[0]]);
    assert_eq!(
        repo.unstaged().len(),
        1,
        "the line-18 edit is unstaged again"
    );
    assert_eq!(
        repo.worktree_text(),
        edited(),
        "the working tree is untouched"
    );
}

#[test]
fn a_hunk_drawn_before_the_file_moved_is_refused() {
    let repo = Repo::new();
    let drawn = repo.unstaged();
    // Two lines inserted above: both hunks move down, so neither drawn header
    // exists any more.
    std::fs::write(
        repo.path().join(FILE),
        format!("new a\nnew b\n{}", edited()),
    )
    .unwrap();

    let err = repo
        .backend()
        .stage_hunk(Path::new(FILE), drawn[1])
        .expect_err("a moved hunk must be refused");
    assert!(is_hunk_changed(&err), "got {err:?}");
    assert_eq!(repo.index_text(), original(), "the refusal wrote nothing");
}

#[test]
fn an_unstage_of_a_hunk_no_longer_staged_is_refused() {
    let repo = Repo::new();
    let hunks = repo.unstaged();
    repo.backend()
        .stage_hunk(Path::new(FILE), hunks[1])
        .unwrap();
    let drawn = repo.staged();
    // The index moves under the drawn staged diff: the line-2 edit joins it,
    // so the staged hunk that was drawn is still there, but a range that only
    // existed before is not.
    repo.backend()
        .stage_hunk(Path::new(FILE), hunks[0])
        .unwrap();
    let stale = HunkRange {
        old: (drawn[0].old.0 + 1, drawn[0].old.1),
        new: drawn[0].new,
    };
    let before = repo.index_text();
    let err = repo
        .backend()
        .unstage_hunk(Path::new(FILE), stale)
        .expect_err("a staged hunk that is not in the index diff is refused");
    assert!(is_hunk_changed(&err), "got {err:?}");
    assert_eq!(repo.index_text(), before, "the refusal wrote nothing");
}

#[test]
fn an_untracked_file_is_its_one_hunk() {
    let repo = Repo::new();
    std::fs::write(repo.path().join("new.txt"), "a\nb\n").unwrap();
    let backend = repo.backend();
    let diff = backend.unstaged_file_diff(Path::new("new.txt")).unwrap();
    assert_eq!(diff.hunks.len(), 1);

    backend
        .stage_hunk(Path::new("new.txt"), diff.hunks[0].range())
        .expect("stage the whole new file");
    assert_eq!(show_index(repo.path(), "new.txt"), "a\nb\n");

    let staged = backend.staged_file_diff(Path::new("new.txt")).unwrap();
    backend
        .unstage_hunk(Path::new("new.txt"), staged.hunks[0].range())
        .expect("unstage the whole new file");
    let listed = git_output(repo.path(), &["ls-files", "--", "new.txt"]);
    assert!(listed.is_empty(), "new.txt is untracked again");
}
