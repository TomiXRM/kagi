//! Pull previews must use the configured upstream even when origin/<local-name>
//! exists. All commits, fetches and conflict witnesses use local Git repositories.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kagi_domain::plan_note::{PlanNote, PlanTitle, PullNote, PullTitle};
use kagi_git::{Backend, OpOutcome, Operation, OperationOutcome, PullOutcome};

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{
    commit_all, git, git_output, git_succeeds, init_repo, repo_with_bare_origin, write_file,
    RemoteFixture,
};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

struct Fixture {
    repos: RemoteFixture,
    producer: PathBuf,
    base: String,
    alternate: String,
}

impl Fixture {
    fn new() -> Self {
        let repos = repo_with_bare_origin("main");
        let base = git_output(&repos.local, &["rev-parse", "HEAD"]);
        let producer = repos.local.parent().unwrap().join("producer");
        std::fs::create_dir(&producer).unwrap();
        init_repo(&producer, "seed");
        git(
            &producer,
            &["remote", "add", "origin", repos.remote.to_str().unwrap()],
        );
        git(&producer, &["fetch", "-q", "origin"]);
        git(
            &producer,
            &["checkout", "-q", "-b", "alternate", "origin/main"],
        );
        write_file(&producer, "a.txt", "configured upstream edit\n");
        write_file(&producer, "alternate.txt", "only on configured upstream\n");
        commit_all(&producer, "advance alternate, leave main untouched");
        git(&producer, &["push", "-q", "origin", "alternate"]);
        let alternate = git_output(&producer, &["rev-parse", "HEAD"]);
        git(&repos.local, &["fetch", "-q", "origin"]);
        git(
            &repos.local,
            &["branch", "--set-upstream-to=origin/alternate", "main"],
        );
        assert_eq!(
            git_output(&repos.local, &["rev-parse", "origin/main"]),
            base
        );
        assert_ne!(base, alternate);
        Self {
            repos,
            producer,
            base,
            alternate,
        }
    }

    fn local(&self) -> &Path {
        &self.repos.local
    }

    fn backend(&self) -> Backend {
        Backend::open_with_policy(
            self.local(),
            kagi_git::backend::ExecutionPolicy::human(false),
        )
        .expect("open backend")
    }
}

#[derive(Debug, PartialEq, Eq)]
struct CheckoutState {
    head: String,
    symbolic_head: String,
    index: Vec<u8>,
    entries: String,
    worktree: BTreeMap<PathBuf, Vec<u8>>,
}

fn collect_files(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            collect_files(root, &path, files);
        } else {
            files.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                std::fs::read(path).unwrap(),
            );
        }
    }
}

fn checkout_state(local: &Path) -> CheckoutState {
    let mut worktree = BTreeMap::new();
    collect_files(local, local, &mut worktree);
    CheckoutState {
        head: git_output(local, &["rev-parse", "HEAD"]),
        symbolic_head: git_output(local, &["symbolic-ref", "HEAD"]),
        index: std::fs::read(local.join(".git/index")).unwrap(),
        entries: git_output(local, &["ls-files", "--stage"]),
        worktree,
    }
}

#[test]
fn public_pull_plans_preserve_exact_configured_remote_and_upstream() {
    if !test_support::run_isolated() {
        return;
    }
    for remote in ["origin", "team", "team/origin", "."] {
        let fixture = Fixture::new();
        let upstream_ref = if remote == "." {
            git(
                fixture.local(),
                &["branch", "alternate", &fixture.alternate],
            );
            git(fixture.local(), &["config", "branch.main.remote", "."]);
            git(
                fixture.local(),
                &["config", "branch.main.merge", "refs/heads/alternate"],
            );
            "refs/heads/alternate".to_string()
        } else {
            if remote != "origin" {
                git(fixture.local(), &["remote", "rename", "origin", remote]);
            }
            format!("refs/remotes/{remote}/alternate")
        };
        let before = checkout_state(fixture.local());
        let refs = git_output(fixture.local(), &["show-ref"]);
        let backend = fixture.backend();
        for op in [
            Operation::Pull,
            Operation::PullBranchFf {
                branch_name: "main".into(),
            },
        ] {
            let plan = backend.plan(&op).expect("plan exact upstream");
            assert!(plan.blockers.is_empty(), "{remote}: {:?}", plan.blockers);
            let (branch, title_remote, behind) = match &plan.title {
                PlanTitle::Pull(PullTitle::Pull {
                    branch,
                    remote,
                    behind,
                })
                | PlanTitle::Pull(PullTitle::PullBranchFf {
                    branch,
                    remote,
                    behind,
                }) => (branch, remote, behind),
                other => panic!("unexpected pull title: {other:?}"),
            };
            assert_eq!(branch, "main");
            assert_eq!(title_remote, remote);
            assert_eq!(*behind, 1);
            let identity = plan.pull_identity.as_ref().expect("approved identity");
            assert_eq!(identity.branch, "main");
            assert_eq!(identity.remote, remote);
            assert_eq!(identity.upstream_ref, upstream_ref);
            assert_eq!(identity.local_oid.0, fixture.base);
            assert_eq!(checkout_state(fixture.local()), before);
            assert_eq!(git_output(fixture.local(), &["show-ref"]), refs);
        }
    }
}

#[test]
fn public_pull_plans_keep_missing_upstream_blocked_without_identity() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    git(
        fixture.local(),
        &["remote", "rename", "origin", "team/origin"],
    );
    git(
        fixture.local(),
        &["update-ref", "-d", "refs/remotes/team/origin/alternate"],
    );
    let before = checkout_state(fixture.local());
    let backend = fixture.backend();
    for op in [
        Operation::Pull,
        Operation::PullBranchFf {
            branch_name: "main".into(),
        },
    ] {
        let plan = backend.plan(&op).expect("blocked upstream plan");
        assert!(plan.pull_identity.is_none());
        assert!(plan.blockers.iter().any(|note| matches!(
            note,
            PlanNote::Pull(PullNote::NoUpstream { branch, err })
                | PlanNote::Pull(PullNote::NoUpstreamWithHint { branch, err })
                if branch == "main" && !err.is_empty()
        )));
        assert_eq!(checkout_state(fixture.local()), before);
    }
}

#[test]
fn dirty_restore_warning_names_conflict_on_configured_alternate() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    write_file(fixture.local(), "a.txt", "user working edit\n");
    write_file(fixture.local(), "staged.txt", "preserve staged addition\n");
    git(fixture.local(), &["add", "staged.txt"]);
    write_file(
        fixture.local(),
        "untracked.txt",
        "preserve untracked file\n",
    );
    let before = checkout_state(fixture.local());
    let refs = git_output(fixture.local(), &["show-ref"]);
    let mut backend = fixture.backend();
    let plan = backend.plan(&Operation::Pull).expect("plan dirty pull");
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let paths = plan.warnings.iter().find_map(|note| match note {
        PlanNote::Pull(PullNote::RestoreConflict { paths }) => Some(paths.as_slice()),
        _ => None,
    });
    assert_eq!(
        paths,
        Some(["a.txt".to_string()].as_slice()),
        "{:?}",
        plan.warnings
    );
    assert_eq!(
        checkout_state(fixture.local()),
        before,
        "preview must not write"
    );
    assert_eq!(git_output(fixture.local(), &["show-ref"]), refs);

    let report = backend.run_recorded(&Operation::Pull, &plan);
    assert!(
        report.result.is_err(),
        "overlapping dirt must not be overwritten"
    );
    assert!(matches!(
        report.recording,
        kagi_git::backend::recording::Recording::Appended { .. }
    ));
    assert!(matches!(
        &report.recording.entry().outcome,
        OpOutcome::Failed { .. }
    ));
    assert_eq!(
        checkout_state(fixture.local()),
        before,
        "refusal preserves staging and files"
    );
    assert_eq!(git_output(fixture.local(), &["show-ref"]), refs);

    // Independently witness the promised restore conflict with actual Git:
    // stash the same user edit at the base, FF to alternate, then restore it.
    git(
        &fixture.producer,
        &["checkout", "-q", "-b", "witness", &fixture.base],
    );
    write_file(&fixture.producer, "a.txt", "user working edit\n");
    git(&fixture.producer, &["stash", "push", "-q"]);
    git(
        &fixture.producer,
        &["merge", "-q", "--ff-only", "alternate"],
    );
    assert!(!git_succeeds(&fixture.producer, &["stash", "pop", "-q"]));
    assert!(git_output(&fixture.producer, &["ls-files", "--unmerged"]).contains("a.txt"));
}

#[test]
fn diverged_merge_warning_uses_configured_alternate_and_preserves_checkout() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    write_file(fixture.local(), "a.txt", "local committed edit\n");
    commit_all(fixture.local(), "local divergence");
    // Keep the misleading same-name tracker exactly at HEAD, so it predicts
    // no incoming change while the real configured upstream conflicts.
    git(fixture.local(), &["push", "-q", "origin", "main"]);
    assert_eq!(
        git_output(fixture.local(), &["rev-parse", "origin/main"]),
        git_output(fixture.local(), &["rev-parse", "HEAD"])
    );
    let before = checkout_state(fixture.local());
    let refs = git_output(fixture.local(), &["show-ref"]);
    let mut backend = fixture.backend();
    let plan = backend.plan(&Operation::Pull).expect("plan diverged pull");
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert!(
        plan.warnings
            .iter()
            .any(|note| matches!(note, PlanNote::Pull(PullNote::MergePrediction))),
        "configured upstream must predict the real merge conflict: {:?}",
        plan.warnings
    );
    assert_eq!(checkout_state(fixture.local()), before);
    let report = backend.run_recorded(&Operation::Pull, &plan);
    assert!(
        report.result.is_err(),
        "conflicting merge must refuse before checkout"
    );
    assert!(matches!(
        report.recording,
        kagi_git::backend::recording::Recording::Appended { .. }
    ));
    assert!(matches!(
        &report.recording.entry().outcome,
        OpOutcome::Failed { .. }
    ));
    assert_eq!(checkout_state(fixture.local()), before);
    assert_eq!(git_output(fixture.local(), &["show-ref"]), refs);
    assert!(!fixture.local().join(".git/MERGE_HEAD").exists());
    assert!(git_output(fixture.local(), &["ls-files", "--unmerged"]).is_empty());
}

#[test]
fn noncurrent_ff_previews_configured_alternate_and_fetches_newer_approved_tip() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    git(fixture.local(), &["checkout", "-q", "-b", "work"]);
    write_file(fixture.local(), "a.txt", "work branch unstaged edit\n");
    write_file(fixture.local(), "staged.txt", "work branch staged edit\n");
    git(fixture.local(), &["add", "staged.txt"]);
    write_file(
        fixture.local(),
        "untracked.txt",
        "work branch untracked file\n",
    );
    let before = checkout_state(fixture.local());
    let mut backend = fixture.backend();
    let op = Operation::PullBranchFf {
        branch_name: "main".into(),
    };
    let plan = backend.plan(&op).expect("plan ref-only pull");
    assert!(
        plan.blockers.is_empty(),
        "main is behind its configured alternate, not already up to date: {:?}",
        plan.blockers
    );
    assert!(
        plan.predicted.head.contains(&fixture.alternate[..8]),
        "{:?}",
        plan.predicted
    );
    assert_eq!(checkout_state(fixture.local()), before);
    assert_eq!(
        git_output(fixture.local(), &["rev-parse", "main"]),
        fixture.base
    );

    // Approval freezes the upstream identity, not the fetched commit. A newer
    // commit on that same alternate must be discovered and remain permitted.
    write_file(&fixture.producer, "newer.txt", "arrived after approval\n");
    commit_all(
        &fixture.producer,
        "advance approved alternate after planning",
    );
    git(&fixture.producer, &["push", "-q", "origin", "alternate"]);
    let newer = git_output(&fixture.producer, &["rev-parse", "HEAD"]);
    assert_eq!(
        git_output(fixture.local(), &["rev-parse", "origin/alternate"]),
        fixture.alternate
    );
    let report = backend.run_recorded(&op, &plan);
    assert!(
        matches!(
            &report.result,
            Ok(OperationOutcome::Pull(PullOutcome::FastForward { .. }))
        ),
        "{:?}",
        report.result
    );
    assert!(matches!(
        report.recording,
        kagi_git::backend::recording::Recording::Appended { .. }
    ));
    assert!(matches!(
        &report.recording.entry().outcome,
        OpOutcome::Success { .. }
    ));
    assert_eq!(report.recording.entry().op, "pull");
    assert_eq!(git_output(fixture.local(), &["rev-parse", "main"]), newer);
    assert_eq!(
        git_output(fixture.local(), &["rev-parse", "origin/alternate"]),
        newer
    );
    assert_eq!(
        git_output(fixture.local(), &["rev-parse", "origin/main"]),
        fixture.base
    );
    assert_eq!(
        git_output(fixture.local(), &["show", "main:a.txt"]),
        "configured upstream edit"
    );
    assert_eq!(
        git_output(fixture.local(), &["show", "main:newer.txt"]),
        "arrived after approval"
    );
    assert_eq!(
        checkout_state(fixture.local()),
        before,
        "only the noncurrent target ref may move"
    );
}
