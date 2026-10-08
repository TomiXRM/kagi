//! A pull approval names an upstream, not merely a cached commit or behind count.
//! These fixtures use real local bare remotes and the public Backend boundary.
//! No assertions depend on the plan's pull-identity implementation or fields.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kagi_git::{Backend, OpOutcome, Operation, OperationOutcome, OperationPlan, PullOutcome};

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{
    commit_all, git, git_output, init_repo, repo_with_bare_origin, write_file, RemoteFixture,
};

#[path = "../../../tests/support/isolated.rs"]
mod test_support;

struct Fixture {
    repos: RemoteFixture,
    producer: PathBuf,
    branch: &'static str,
    origin_tip: String,
    fork_tip: String,
}

impl Fixture {
    fn new(current: bool) -> Self {
        let repos = repo_with_bare_origin("main");
        let root = repos.local.parent().unwrap();
        let producer = root.join("producer");
        let fork = root.join("fork.git");
        git(
            root,
            &["init", "-q", "--bare", "-b", "main", fork.to_str().unwrap()],
        );
        std::fs::create_dir(&producer).unwrap();
        init_repo(&producer, "seed");
        git(
            &producer,
            &["remote", "add", "origin", repos.remote.to_str().unwrap()],
        );
        git(
            &producer,
            &["remote", "add", "fork", fork.to_str().unwrap()],
        );
        git(&producer, &["fetch", "-q", "origin"]);
        git(&producer, &["checkout", "-q", "-b", "main", "origin/main"]);

        let branch = if current { "main" } else { "feature" };
        if !current {
            git(&repos.local, &["branch", "feature"]);
            git(&repos.local, &["push", "-q", "-u", "origin", "feature"]);
        }

        write_file(&producer, "a.txt", "approved origin\n");
        write_file(&producer, "origin.txt", "approved origin file\n");
        commit_all(&producer, "origin advance");
        git(
            &producer,
            &["push", "-q", "origin", &format!("main:{branch}")],
        );
        // Both cached tracking branches have exactly the same tip and distance.
        git(&producer, &["push", "-q", "origin", "main:alternate"]);
        let origin_tip = git_output(&producer, &["rev-parse", "HEAD"]);

        write_file(&producer, "a.txt", "unapproved fork\n");
        write_file(&producer, "fork.txt", "unapproved fork file\n");
        commit_all(&producer, "fork advance");
        git(
            &producer,
            &["push", "-q", "fork", &format!("main:{branch}")],
        );
        let fork_tip = git_output(&producer, &["rev-parse", "HEAD"]);
        git(
            &repos.local,
            &["remote", "add", "fork", fork.to_str().unwrap()],
        );
        git(&repos.local, &["fetch", "-q", "origin"]);
        git(&repos.local, &["fetch", "-q", "fork"]);
        // The alternate remote is deliberately FF-able; refusal must be about
        // approval identity, not a divergence or checkout-safety blocker.
        git(
            &repos.local,
            &[
                "merge-base",
                "--is-ancestor",
                branch,
                &format!("fork/{branch}"),
            ],
        );

        Self {
            repos,
            producer,
            branch,
            origin_tip,
            fork_tip,
        }
    }

    fn local(&self) -> &Path {
        &self.repos.local
    }

    fn op(&self, current: bool) -> Operation {
        if current {
            Operation::Pull
        } else {
            Operation::PullBranchFf {
                branch_name: self.branch.into(),
            }
        }
    }

    fn backend(&self) -> Backend {
        Backend::open_with_policy(
            self.local(),
            kagi_git::backend::ExecutionPolicy::human(false),
        )
        .expect("open backend")
    }

    fn plan(&self, backend: &Backend, op: &Operation) -> OperationPlan {
        let plan = backend.plan(op).expect("plan pull");
        assert!(
            plan.blockers.is_empty(),
            "fixture pull must be allowed: {:?}",
            plan.blockers
        );
        assert_ne!(
            git_output(self.local(), &["rev-parse", self.branch]),
            self.origin_tip
        );
        plan
    }
}

#[derive(Debug, PartialEq, Eq)]
struct CheckoutState {
    head: String,
    symbolic_head: String,
    index_bytes: Vec<u8>,
    index_entries: String,
    index_blob: String,
    worktree: BTreeMap<PathBuf, Vec<u8>>,
}

fn worktree_files(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path == root.join(".git") {
            continue;
        }
        if path.is_dir() {
            worktree_files(root, &path, files);
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
    worktree_files(local, local, &mut worktree);
    CheckoutState {
        head: git_output(local, &["rev-parse", "HEAD"]),
        symbolic_head: git_output(local, &["symbolic-ref", "HEAD"]),
        index_bytes: std::fs::read(local.join(".git/index")).unwrap(),
        index_entries: git_output(local, &["ls-files", "--stage"]),
        index_blob: git_output(local, &["show", ":a.txt"]),
        worktree,
    }
}

fn assert_refused_without_mutation(current: bool, same_remote: bool) {
    let fixture = Fixture::new(current);
    let mut backend = fixture.backend();
    let op = fixture.op(current);
    let plan = fixture.plan(&backend, &op);
    let before = checkout_state(fixture.local());
    let branch_before = git_output(fixture.local(), &["rev-parse", fixture.branch]);
    let branch_blob_before = git_output(
        fixture.local(),
        &["show", &format!("{}:a.txt", fixture.branch)],
    );

    let upstream = if same_remote {
        "origin/alternate".to_string()
    } else {
        format!("fork/{}", fixture.branch)
    };
    if same_remote {
        assert_eq!(
            git_output(fixture.local(), &["rev-parse", &upstream]),
            fixture.origin_tip
        );
        assert_eq!(
            git_output(
                fixture.local(),
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("{}...origin/{}", fixture.branch, fixture.branch)
                ]
            ),
            git_output(
                fixture.local(),
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("{}...{upstream}", fixture.branch)
                ]
            ),
            "cached OID and ahead/behind counts cannot distinguish these upstreams",
        );
    } else {
        assert_eq!(
            git_output(fixture.local(), &["rev-parse", &upstream]),
            fixture.fork_tip
        );
    }
    git(
        fixture.local(),
        &["branch", "--set-upstream-to", &upstream, fixture.branch],
    );
    assert_eq!(
        checkout_state(fixture.local()),
        before,
        "changing upstream is config-only"
    );
    if same_remote {
        // Cached tips still match, but the unapproved tracker advances remotely.
        // Old title/count checks would accept and fetch this different content.
        git(
            &fixture.producer,
            &["push", "-q", "origin", "main:alternate"],
        );
    }

    let report = backend.run_recorded(&op, &plan);
    assert!(
        report.result.is_err(),
        "a pull approved for origin/{} must refuse the changed upstream {upstream}: {:?}",
        fixture.branch,
        report.result
    );
    assert!(
        matches!(
            &report.recording,
            kagi_git::backend::recording::Recording::Appended { .. }
        ),
        "refusal receipt must be persisted: {:?}",
        report.recording
    );
    let receipt = report.recording.entry();
    assert_eq!(receipt.op, "pull");
    assert_eq!(
        std::fs::canonicalize(&receipt.repo).unwrap(),
        std::fs::canonicalize(fixture.local()).unwrap()
    );
    assert!(
        matches!(&receipt.outcome, OpOutcome::Failed { .. }),
        "refusal must have a failed receipt: {:?}",
        receipt.outcome
    );
    assert_eq!(
        git_output(fixture.local(), &["rev-parse", fixture.branch]),
        branch_before,
        "target ref must not move"
    );
    assert_eq!(
        git_output(
            fixture.local(),
            &["show", &format!("{}:a.txt", fixture.branch)]
        ),
        branch_blob_before,
        "target blob must stay approved-local"
    );
    assert_eq!(
        checkout_state(fixture.local()),
        before,
        "refusal must preserve HEAD, index bytes/blobs, and every worktree file"
    );
}

fn assert_newer_same_upstream_allowed(current: bool) {
    let fixture = Fixture::new(current);
    let mut backend = fixture.backend();
    let op = fixture.op(current);
    let plan = fixture.plan(&backend, &op);
    let before = checkout_state(fixture.local());

    // Publish a newer descendant only AFTER approval, without fetching locally.
    // fork_tip is already in the producer's history; the approved origin now
    // advances beyond it. Pull must fetch the new origin tip, not freeze its
    // cached OID at plan time (nor reject it merely because that OID changed).
    write_file(&fixture.producer, "a.txt", "newer approved upstream\n");
    write_file(
        &fixture.producer,
        "newer.txt",
        "discovered by execution fetch\n",
    );
    commit_all(&fixture.producer, "origin advances after approval");
    git(
        &fixture.producer,
        &["push", "-q", "origin", &format!("main:{}", fixture.branch)],
    );
    let newer_tip = git_output(&fixture.producer, &["rev-parse", "HEAD"]);
    assert_eq!(
        git_output(
            fixture.local(),
            &["rev-parse", &format!("origin/{}", fixture.branch)]
        ),
        fixture.origin_tip,
        "local tracker must still have the approved cached tip"
    );

    let report = backend.run_recorded(&op, &plan);
    assert!(
        matches!(
            &report.result,
            Ok(OperationOutcome::Pull(PullOutcome::FastForward { .. }))
        ),
        "same-upstream advancement must remain allowed: {:?}",
        report.result
    );
    assert!(
        matches!(
            &report.recording,
            kagi_git::backend::recording::Recording::Appended { .. }
        ),
        "success receipt must be persisted: {:?}",
        report.recording
    );
    let receipt = report.recording.entry();
    assert_eq!(receipt.op, "pull");
    assert_eq!(
        std::fs::canonicalize(&receipt.repo).unwrap(),
        std::fs::canonicalize(fixture.local()).unwrap()
    );
    assert!(matches!(&receipt.outcome, OpOutcome::Success { .. }));
    assert_eq!(
        git_output(fixture.local(), &["rev-parse", fixture.branch]),
        newer_tip
    );
    assert_eq!(
        git_output(
            fixture.local(),
            &["rev-parse", &format!("origin/{}", fixture.branch)]
        ),
        newer_tip,
        "execution fetch must discover the newer approved tip"
    );
    assert_eq!(
        git_output(
            fixture.local(),
            &["show", &format!("{}:a.txt", fixture.branch)]
        ),
        "newer approved upstream"
    );
    if current {
        assert_eq!(
            git_output(fixture.local(), &["rev-parse", "HEAD"]),
            newer_tip
        );
        assert_eq!(
            git_output(fixture.local(), &["symbolic-ref", "HEAD"]),
            before.symbolic_head
        );
        assert_eq!(
            git_output(fixture.local(), &["write-tree"]),
            git_output(fixture.local(), &["rev-parse", "HEAD^{tree}"]),
            "index tree must match the new HEAD"
        );
        assert_eq!(
            git_output(fixture.local(), &["show", ":a.txt"]),
            "newer approved upstream"
        );
        assert_eq!(
            std::fs::read(fixture.local().join("a.txt")).unwrap(),
            b"newer approved upstream\n"
        );
        assert_eq!(
            std::fs::read(fixture.local().join("newer.txt")).unwrap(),
            b"discovered by execution fetch\n"
        );
        assert!(
            git_output(fixture.local(), &["status", "--porcelain"]).is_empty(),
            "checkout/index/worktree must agree after the successful pull"
        );
    } else {
        assert_eq!(
            checkout_state(fixture.local()),
            before,
            "non-current pull must update only the target ref, not HEAD/index/worktree"
        );
    }
}

#[test]
fn non_current_pull_rejects_changed_remote_without_mutation() {
    if !test_support::run_isolated() {
        return;
    }
    assert_refused_without_mutation(false, false);
}

#[test]
fn current_pull_rejects_changed_remote_without_mutation() {
    if !test_support::run_isolated() {
        return;
    }
    assert_refused_without_mutation(true, false);
}

#[test]
fn non_current_pull_rejects_same_oid_different_tracking_branch() {
    if !test_support::run_isolated() {
        return;
    }
    assert_refused_without_mutation(false, true);
}

#[test]
fn current_pull_rejects_same_oid_different_tracking_branch() {
    if !test_support::run_isolated() {
        return;
    }
    assert_refused_without_mutation(true, true);
}

#[test]
fn non_current_pull_fetches_newer_same_upstream_after_approval() {
    if !test_support::run_isolated() {
        return;
    }
    assert_newer_same_upstream_allowed(false);
}

#[test]
fn current_pull_fetches_newer_same_upstream_after_approval() {
    if !test_support::run_isolated() {
        return;
    }
    assert_newer_same_upstream_allowed(true);
}
