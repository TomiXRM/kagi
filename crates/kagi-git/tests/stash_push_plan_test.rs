//! Stash Push approval must explain refusals and predict the files it leaves behind.
use kagi_git::{Backend, GitError, Operation};
use std::path::{Path, PathBuf};

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod test_support;
use git_fixture::{git, git_output, init_repo};

fn files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn collect(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(root, &path, out);
            } else {
                out.push((
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    collect(root, root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn stash_push_unborn_plan_refuses_without_writes() {
    if !test_support::run_isolated() {
        return;
    }
    for state in ["empty", "untracked", "staged"] {
        let fixture = tempfile::tempdir().unwrap();
        let repo = fixture.path();
        init_repo(repo, "main");
        if state != "empty" {
            std::fs::write(repo.join("new"), "keep\n").unwrap();
        }
        if state == "staged" {
            git(repo, &["add", "new"]);
        }
        let before = files(repo);
        let mut backend = Backend::open(repo).unwrap();
        let op = Operation::StashPush {
            message: None,
            include_untracked: true,
        };
        let plan = backend.plan(&op).unwrap();
        assert!(
            plan.blockers.iter().any(|note| note.message_en()
                == "Stash push requires a HEAD commit. Create an initial commit before stashing."),
            "{state}: unborn HEAD must be explained in the plan: {:?}",
            plan.blockers
        );
        assert!(matches!(backend.preflight_check_stash(&plan, 0),
            Err(GitError::Blocked(note)) if note.message_en().contains("HEAD commit")));
        assert_eq!(
            plan.predicted, plan.current,
            "a blocked push predicts no change"
        );
        assert!(matches!(backend.run(&op, &plan),
            Err(GitError::Other(message)) if message == "plan has blockers"));
        let entries = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100);
        assert_eq!(entries.len(), 1);
        assert!(
            matches!(&entries[0].outcome, kagi_git::oplog::OpOutcome::Refused { blockers }
            if blockers.iter().any(|note| note.contains("HEAD commit")))
        );
        assert_eq!(
            files(repo),
            before,
            "{state}: refusal cannot write any repository file"
        );
        assert!(!repo.join(".git/refs/stash").exists());
        assert_eq!(git_output(repo, &["stash", "list"]), "");
    }
}

#[test]
fn stash_push_preflight_rechecks_live_head_commit() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    init_repo(repo, "main");
    std::fs::write(repo.join("tracked"), "base\n").unwrap();
    git_fixture::commit_all(repo, "base");
    std::fs::write(repo.join("tracked"), "dirty\n").unwrap();
    let mut backend = Backend::open(repo).unwrap();
    let op = Operation::StashPush {
        message: None,
        include_untracked: true,
    };
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty());
    // Switch only HEAD to an unborn branch; leave the approved index/files.
    git(repo, &["symbolic-ref", "HEAD", "refs/heads/unborn"]);
    let before = files(repo);
    assert!(matches!(backend.preflight_check_stash(&plan, 0),
        Err(GitError::Blocked(note)) if note.message_en().contains("HEAD commit")));
    assert_eq!(files(repo), before);
    assert!(!repo.join(".git/refs/stash").exists());
}

#[test]
fn stash_push_prediction_matches_retained_untracked() {
    if !test_support::run_isolated() {
        return;
    }
    for tracked in ["modified", "staged", "both", "none", "deleted_untracked"] {
        for untracked in [0, 2] {
            for include_untracked in [false, true] {
                let fixture = tempfile::tempdir().unwrap();
                let repo = fixture.path();
                init_repo(repo, "main");
                std::fs::write(repo.join("tracked"), "base\n").unwrap();
                git_fixture::commit_all(repo, "base");
                // Neither supported push mode includes ignored files.
                std::fs::write(repo.join(".git/info/exclude"), "ignored\n").unwrap();
                std::fs::write(repo.join("ignored"), "ignored bytes\n").unwrap();
                if tracked != "none" {
                    std::fs::write(repo.join("tracked"), "change\n").unwrap();
                    if tracked == "staged" || tracked == "both" {
                        git(repo, &["add", "tracked"]);
                    }
                    if tracked == "both" {
                        std::fs::write(repo.join("tracked"), "second change\n").unwrap();
                    }
                    if tracked == "deleted_untracked" {
                        git(repo, &["rm", "--cached", "tracked"]);
                    }
                }
                for n in 0..untracked {
                    std::fs::write(repo.join(format!("new{n}")), "keep\n").unwrap();
                }
                let mut backend = Backend::open(repo).unwrap();
                let op = Operation::StashPush {
                    message: None,
                    include_untracked,
                };
                let plan = backend.plan(&op).unwrap();
                if tracked == "none" && (!include_untracked || untracked == 0) {
                    assert!(!plan.blockers.is_empty());
                    assert_eq!(plan.predicted, plan.current);
                    assert!(backend.run(&op, &plan).is_err());
                    assert!(!repo.join(".git/refs/stash").exists());
                    continue;
                }
                assert!(plan.blockers.is_empty());
                let retained = if include_untracked { 0 } else { untracked };
                let expected = if retained == 0 {
                    "clean".to_owned()
                } else {
                    format!("{retained} untracked retained")
                };
                assert_eq!(
                    plan.predicted.dirty, expected,
                    "{tracked}, untracked={untracked}, include={include_untracked}"
                );
                if !include_untracked {
                    let excluded = plan.warnings.iter().find_map(|note| match note {
                        kagi_domain::plan_note::PlanNote::Stash(
                            kagi_domain::plan_note::StashNote::UntrackedExcluded { count },
                        ) => Some(*count),
                        _ => None,
                    });
                    assert_eq!(excluded, (retained > 0).then_some(retained));
                }
                backend.run(&op, &plan).unwrap();
                let status = git_output(repo, &["status", "--porcelain"]);
                assert_eq!(status.lines().count(), retained);
                assert!(status.lines().all(|line| line.starts_with("?? ")));
                let after = backend.working_tree_status().unwrap();
                assert!(
                    after.staged.is_empty()
                        && after.unstaged.is_empty()
                        && after.conflicted.is_empty()
                );
                assert_eq!(after.untracked.len(), retained);
                assert_eq!(
                    std::fs::read_to_string(repo.join("ignored")).unwrap(),
                    "ignored bytes\n"
                );
                assert_eq!(
                    git_output(repo, &["stash", "list", "--format=%H"])
                        .lines()
                        .count(),
                    1
                );
                let entries = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100);
                assert_eq!(entries.len(), 1);
                assert!(matches!(
                    entries[0].outcome,
                    kagi_git::oplog::OpOutcome::Success { .. }
                ));
            }
        }
    }
}
