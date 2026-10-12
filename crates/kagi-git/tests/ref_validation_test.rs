//! Ref creation and upstream plans reject unexecutable names before confirmation.
#[path = "../../../tests/support/backend_ops.rs"]
mod backend_ops;
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

use git2::Repository;
use kagi_domain::plan_note::PlanDisposition;
use kagi_git::ops::{plan_create_branch, plan_create_tag, plan_set_upstream};
use kagi_git::CommitId;
use tempfile::TempDir;

fn fixture() -> (TempDir, Repository, CommitId) {
    let tmp = TempDir::new().unwrap();
    git_fixture::init_repo(tmp.path(), "main");
    git_fixture::write_file(tmp.path(), "base.txt", "base\n");
    git_fixture::git(tmp.path(), &["add", "base.txt"]);
    git_fixture::git(tmp.path(), &["commit", "-qm", "base"]);
    let head = CommitId(git_fixture::git_output(tmp.path(), &["rev-parse", "HEAD"]));
    let repo = Repository::open(tmp.path()).unwrap();
    (tmp, repo, head)
}

#[test]
fn create_ref_collision_blockers() {
    if !test_support::run_isolated() {
        return;
    }
    for namespace in ["heads", "tags"] {
        for (existing, requested) in [
            ("feature", "feature"),
            ("feature", "feature/x"),
            ("feature/x", "feature"),
        ] {
            for packed in [false, true] {
                let (tmp, repo, head) = fixture();
                let oid = git2::Oid::from_str(&head.0).unwrap();
                repo.reference(
                    &format!("refs/{namespace}/{existing}"),
                    oid,
                    false,
                    "fixture",
                )
                .unwrap();
                if packed {
                    git_fixture::git(tmp.path(), &["pack-refs", "--all"]);
                }
                let before = git_fixture::git_output(tmp.path(), &["show-ref"]);
                let plan = if namespace == "heads" {
                    plan_create_branch(&repo, requested, &head)
                } else {
                    plan_create_tag(&repo, requested, &head)
                }
                .unwrap();
                assert_eq!(
                    plan.disposition,
                    PlanDisposition::Blocked,
                    "{namespace}/{requested}, existing {existing}, packed {packed}"
                );
                assert!(!plan.blockers.is_empty());
                assert_eq!(git_fixture::git_output(tmp.path(), &["show-ref"]), before);
            }
        }
    }
}

#[test]
fn create_ref_invalid_names_and_separate_namespaces() {
    if !test_support::run_isolated() {
        return;
    }
    let (_tmp, repo, head) = fixture();
    for name in [
        "", "a..b", "a b", "a@{b", "a.lock", "a/", "a//b", ".hidden", "-flag", "a\u{7f}",
    ] {
        assert_eq!(
            plan_create_branch(&repo, name, &head).unwrap().disposition,
            PlanDisposition::Blocked,
            "branch {name:?}"
        );
        assert_eq!(
            plan_create_tag(&repo, name, &head).unwrap().disposition,
            PlanDisposition::Blocked,
            "tag {name:?}"
        );
    }
    let oid = git2::Oid::from_str(&head.0).unwrap();
    repo.reference("refs/tags/shared", oid, false, "fixture")
        .unwrap();
    repo.reference("refs/heads/other", oid, false, "fixture")
        .unwrap();
    assert_eq!(
        plan_create_branch(&repo, "shared/x", &head)
            .unwrap()
            .disposition,
        PlanDisposition::Ready
    );
    assert_eq!(
        plan_create_tag(&repo, "other/x", &head)
            .unwrap()
            .disposition,
        PlanDisposition::Ready
    );
}

#[test]
fn set_upstream_missing_blocker() {
    if !test_support::run_isolated() {
        return;
    }
    let (tmp, repo, head) = fixture();
    git_fixture::git(
        tmp.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    let config_before = std::fs::read(tmp.path().join(".git/config")).unwrap();
    let plan = plan_set_upstream(&repo, "main", "origin/missing").unwrap();
    assert_eq!(plan.disposition, PlanDisposition::Blocked);
    assert!(plan.warnings.is_empty());
    assert!(plan.blockers[0]
        .message_en()
        .contains("fetch it before setting upstream"));
    assert_eq!(
        std::fs::read(tmp.path().join(".git/config")).unwrap(),
        config_before
    );
    repo.reference(
        "refs/remotes/origin/main",
        git2::Oid::from_str(&head.0).unwrap(),
        false,
        "fixture",
    )
    .unwrap();
    assert_eq!(
        plan_set_upstream(&repo, "main", "origin/main")
            .unwrap()
            .disposition,
        PlanDisposition::Ready
    );
}

#[test]
fn upstream_existing_local_and_remote_execute_without_worktree_changes() {
    if !test_support::run_isolated() {
        return;
    }
    let (tmp, repo, head) = fixture();
    git_fixture::git(tmp.path(), &["branch", "local-upstream"]);
    git_fixture::git(
        tmp.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    let oid = git2::Oid::from_str(&head.0).unwrap();
    repo.reference("refs/remotes/origin/main", oid, false, "fixture")
        .unwrap();
    let index_before = std::fs::read(tmp.path().join(".git/index")).unwrap();
    for upstream in ["local-upstream", "origin/main"] {
        let plan = plan_set_upstream(&repo, "main", upstream).unwrap();
        assert_eq!(plan.disposition, PlanDisposition::Ready);
        backend_ops::execute_set_upstream(&repo, &plan, "main", upstream).unwrap();
        let branch = repo.find_branch("main", git2::BranchType::Local).unwrap();
        assert_eq!(branch.upstream().unwrap().name().unwrap(), Some(upstream));
        assert_eq!(repo.head().unwrap().target(), Some(oid));
        assert_eq!(
            std::fs::read(tmp.path().join(".git/index")).unwrap(),
            index_before
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("base.txt")).unwrap(),
            "base\n"
        );
    }
}
