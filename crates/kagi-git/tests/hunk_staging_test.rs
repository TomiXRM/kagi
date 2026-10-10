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

use kagi_domain::diff::HunkApproval;
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
    fn unstaged(&self) -> Vec<HunkApproval> {
        let diff = self.backend().unstaged_file_diff(Path::new(FILE)).unwrap();
        diff.hunks.iter().map(|h| h.approval().unwrap()).collect()
    }
    fn staged(&self) -> Vec<HunkApproval> {
        let diff = self.backend().staged_file_diff(Path::new(FILE)).unwrap();
        diff.hunks.iter().map(|h| h.approval().unwrap()).collect()
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
    let mut stale = drawn[0];
    stale.range.old.0 += 1;
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
        .stage_hunk(Path::new("new.txt"), diff.hunks[0].approval().unwrap())
        .expect("stage the whole new file");
    assert_eq!(show_index(repo.path(), "new.txt"), "a\nb\n");

    let staged = backend.staged_file_diff(Path::new("new.txt")).unwrap();
    backend
        .unstage_hunk(Path::new("new.txt"), staged.hunks[0].approval().unwrap())
        .expect("unstage the whole new file");
    let listed = git_output(repo.path(), &["ls-files", "--", "new.txt"]);
    assert!(listed.is_empty(), "new.txt is untracked again");
}

#[test]
fn same_range_different_content_is_refused_for_stage() {
    let repo = Repo::new();
    std::fs::write(repo.path().join(FILE), "one\ntwo\nthree\n").unwrap();
    commit_all(repo.path(), "three-line base");
    std::fs::write(repo.path().join(FILE), "one\nAPPROVED\nthree\n").unwrap();
    let drawn = repo.unstaged()[0];
    std::fs::write(repo.path().join(FILE), "one\nNOT_APPROVED\nthree\n").unwrap();
    assert_eq!(
        repo.unstaged()[0].range,
        drawn.range,
        "the range did not move"
    );
    let snapshot = unchanged_snapshot(&repo);
    let before = repo.index_text();
    let result = repo.backend().stage_hunk(Path::new(FILE), drawn);
    assert_eq!(
        repo.index_text(),
        before,
        "refusal must not stage NOT_APPROVED content; result={result:?}"
    );
    assert!(is_hunk_changed(&result.unwrap_err()));
    assert_refusal_unchanged(&repo, snapshot, "stage");
}

fn unchanged_snapshot(repo: &Repo) -> (String, Vec<u8>, Vec<u8>, usize) {
    (
        git_output(repo.path(), &["rev-parse", "HEAD"]),
        std::fs::read(repo.path().join(".git/index")).unwrap(),
        std::fs::read(repo.path().join(FILE)).unwrap(),
        kagi_git::read_oplog_tail_for_repo(repo.path(), 100).len(),
    )
}

fn assert_refusal_unchanged(repo: &Repo, before: (String, Vec<u8>, Vec<u8>, usize), op: &str) {
    let after = unchanged_snapshot(repo);
    assert_eq!(after.0, before.0, "HEAD unchanged");
    assert_eq!(after.1, before.1, "index bytes unchanged");
    assert_eq!(after.2, before.2, "worktree bytes unchanged");
    assert_eq!(after.3, before.3 + 1, "exactly one receipt");
    let entries = kagi_git::read_oplog_tail_for_repo(repo.path(), 1);
    assert_eq!(entries[0].op, op);
    assert!(
        matches!(&entries[0].outcome, kagi_git::OpOutcome::Refused { blockers }
        if blockers.len() == 1 && blockers[0].contains("has changed since the diff was drawn"))
    );
}

#[test]
fn same_range_different_content_is_refused_for_unstage() {
    let repo = Repo::new();
    std::fs::write(repo.path().join(FILE), "one\ntwo\nthree\n").unwrap();
    commit_all(repo.path(), "three-line base");
    std::fs::write(repo.path().join(FILE), "one\nAPPROVED\nthree\n").unwrap();
    git_output(repo.path(), &["add", "--", FILE]);
    let drawn = repo.staged()[0];
    std::fs::write(repo.path().join(FILE), "one\nNOT_APPROVED\nthree\n").unwrap();
    git_output(repo.path(), &["add", "--", FILE]);
    assert_eq!(
        repo.staged()[0].range,
        drawn.range,
        "the range did not move"
    );
    let before = unchanged_snapshot(&repo);
    let err = repo
        .backend()
        .unstage_hunk(Path::new(FILE), drawn)
        .unwrap_err();
    assert!(is_hunk_changed(&err), "{err:?}");
    assert_refusal_unchanged(&repo, before, "unstage");
}

#[test]
fn context_only_changes_are_refused_for_stage_and_unstage() {
    for staged in [false, true] {
        let repo = Repo::new();
        if staged {
            git_output(repo.path(), &["add", "--", FILE]);
        }
        let drawn = if staged {
            repo.staged()[0]
        } else {
            repo.unstaged()[0]
        };
        // Preserve TWO/EIGHTEEN and their ranges; change only context present
        // identically on both patch sides.
        let base = original().replace("line 1\n", "changed context\n");
        std::fs::write(repo.path().join(FILE), base).unwrap();
        if staged {
            commit_all(repo.path(), "new context in HEAD");
        } else {
            git_output(repo.path(), &["add", "--", FILE]);
        }
        std::fs::write(
            repo.path().join(FILE),
            edited().replace("line 1\n", "changed context\n"),
        )
        .unwrap();
        if staged {
            git_output(repo.path(), &["add", "--", FILE]);
        }
        let current = if staged {
            repo.staged()[0]
        } else {
            repo.unstaged()[0]
        };
        assert_eq!(drawn.range, current.range, "context change preserves range");
        let before = unchanged_snapshot(&repo);
        let result = if staged {
            repo.backend().unstage_hunk(Path::new(FILE), drawn)
        } else {
            repo.backend().stage_hunk(Path::new(FILE), drawn)
        };
        assert!(is_hunk_changed(&result.unwrap_err()));
        assert_refusal_unchanged(&repo, before, if staged { "unstage" } else { "stage" });
    }
}

#[test]
fn missing_final_newline_approvals_apply_in_both_directions() {
    let repo = Repo::new();
    std::fs::write(repo.path().join(FILE), "one\nAPPROVED").unwrap();
    let approved = repo.unstaged()[0];
    repo.backend()
        .stage_hunk(Path::new(FILE), approved)
        .unwrap();
    assert_eq!(repo.index_text(), "one\nAPPROVED");
    let approved = repo.staged()[0];
    repo.backend()
        .unstage_hunk(Path::new(FILE), approved)
        .unwrap();
    assert_eq!(repo.index_text(), original());
    assert_eq!(repo.worktree_text(), "one\nAPPROVED");
}

#[test]
fn raw_byte_and_final_newline_drift_are_refused_in_both_directions() {
    for (approved, changed) in [
        (
            b"one\nAPPROVED\nthree\n".as_slice(),
            b"one\nAPPROVED\nthree".as_slice(),
        ),
        (
            b"one\n\xff\nthree\n".as_slice(),
            b"one\n\xfe\nthree\n".as_slice(),
        ),
    ] {
        for staged in [false, true] {
            let repo = Repo::new();
            std::fs::write(repo.path().join(FILE), "one\ntwo\nthree\n").unwrap();
            commit_all(repo.path(), "three-line base");
            std::fs::write(repo.path().join(FILE), approved).unwrap();
            if staged {
                git_output(repo.path(), &["add", "--", FILE]);
            }
            // Keep the actual long-lived backend open across the external edit.
            let backend = repo.backend();
            let drawn = if staged {
                backend.staged_file_diff(Path::new(FILE)).unwrap()
            } else {
                backend.unstaged_file_diff(Path::new(FILE)).unwrap()
            }
            .hunks[0]
                .approval()
                .unwrap();
            std::fs::write(repo.path().join(FILE), changed).unwrap();
            if staged {
                git_output(repo.path(), &["add", "--", FILE]);
            }
            let current = if staged {
                repo.staged()[0]
            } else {
                repo.unstaged()[0]
            };
            assert_eq!(current.range, drawn.range, "range is unchanged");
            let before = unchanged_snapshot(&repo);
            let result = if staged {
                backend.unstage_hunk(Path::new(FILE), drawn)
            } else {
                backend.stage_hunk(Path::new(FILE), drawn)
            };
            assert!(is_hunk_changed(&result.unwrap_err()));
            assert_refusal_unchanged(&repo, before, if staged { "unstage" } else { "stage" });
        }
    }
}

#[test]
fn unchanged_swapped_lines_unstage_without_rediffing_in_reverse() {
    let repo = Repo::new();
    std::fs::write(repo.path().join(FILE), "x\nA\ny\nz\n").unwrap();
    commit_all(repo.path(), "swap base");
    std::fs::write(repo.path().join(FILE), "x\ny\nA\nz\n").unwrap();
    git_output(repo.path(), &["add", "--", FILE]);
    let approved = repo.staged()[0];
    repo.backend()
        .unstage_hunk(Path::new(FILE), approved)
        .expect("unchanged displayed swap is approved");
    assert_eq!(repo.index_text(), "x\nA\ny\nz\n");
    assert_eq!(repo.worktree_text(), "x\ny\nA\nz\n");
}

#[test]
fn hunk_receipt_uses_human_head_display() {
    let repo = Repo::new();
    let approved = repo.unstaged()[0];
    let report = repo
        .backend()
        .stage_hunk_recorded(Path::new(FILE), approved, false);
    report.result.unwrap();
    let entry = report.recording.entry();
    assert_eq!(entry.before.head, "branch: main");
    let kagi_git::OpOutcome::Success { after } = &entry.outcome else {
        panic!("success receipt")
    };
    assert_eq!(after.head, "branch: main");
}

#[cfg(unix)]
#[test]
fn typechange_hunks_have_no_local_mutation_approval() {
    for from_symlink in [false, true] {
        let repo = Repo::new();
        if from_symlink {
            std::fs::remove_file(repo.path().join(FILE)).unwrap();
            std::os::unix::fs::symlink("old-target", repo.path().join(FILE)).unwrap();
            commit_all(repo.path(), "symlink base");
            std::fs::remove_file(repo.path().join(FILE)).unwrap();
            std::fs::write(repo.path().join(FILE), "new file\n").unwrap();
        } else {
            std::fs::remove_file(repo.path().join(FILE)).unwrap();
            std::os::unix::fs::symlink("new-target", repo.path().join(FILE)).unwrap();
        }
        let backend = repo.backend();
        let diff = backend.unstaged_file_diff(Path::new(FILE)).unwrap();
        assert!(!diff.hunks.is_empty());
        assert!(
            diff.hunks.iter().all(|h| h.approval().is_none()),
            "typechange needs whole-file staging"
        );
        git_output(repo.path(), &["add", "--", FILE]);
        let diff = backend.staged_file_diff(Path::new(FILE)).unwrap();
        assert!(!diff.hunks.is_empty());
        assert!(
            diff.hunks.iter().all(|h| h.approval().is_none()),
            "typechange needs whole-file unstaging"
        );
    }
}

#[cfg(unix)]
#[test]
fn reverse_approval_preserves_quoted_paths_executable_mode_and_missing_lf() {
    use std::os::unix::fs::PermissionsExt;
    let repo = Repo::new();
    let file = Path::new("quoted \" path.txt");
    std::fs::write(repo.path().join(file), "base\n").unwrap();
    std::fs::set_permissions(
        repo.path().join(file),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    commit_all(repo.path(), "quoted executable base");
    std::fs::write(repo.path().join(file), "approved").unwrap();
    let backend = repo.backend();
    let approved = backend.unstaged_file_diff(file).unwrap().hunks[0]
        .approval()
        .unwrap();
    backend.stage_hunk(file, approved).unwrap();
    let approved = backend.staged_file_diff(file).unwrap().hunks[0]
        .approval()
        .unwrap();
    backend.unstage_hunk(file, approved).unwrap();
    assert_eq!(show_index(repo.path(), file.to_str().unwrap()), "base\n");
    assert!(git_output(
        repo.path(),
        &["ls-files", "--stage", "--", file.to_str().unwrap()]
    )
    .starts_with("100755 "));
    assert_eq!(std::fs::read(repo.path().join(file)).unwrap(), b"approved");
}

#[test]
fn a_deleted_file_can_be_unstaged_from_its_approved_hunk() {
    let repo = Repo::new();
    std::fs::remove_file(repo.path().join(FILE)).unwrap();
    let backend = repo.backend();
    let approved = backend.unstaged_file_diff(Path::new(FILE)).unwrap().hunks[0]
        .approval()
        .unwrap();
    backend.stage_hunk(Path::new(FILE), approved).unwrap();
    let approved = backend.staged_file_diff(Path::new(FILE)).unwrap().hunks[0]
        .approval()
        .unwrap();
    backend.unstage_hunk(Path::new(FILE), approved).unwrap();
    assert_eq!(repo.index_text(), original());
    assert!(
        !repo.path().join(FILE).exists(),
        "unstaging never restores the worktree file"
    );
}
