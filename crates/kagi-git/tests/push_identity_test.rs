//! Approved publish plans must never follow a newly selected push destination.
//! Every remote is an independent disposable local bare repository.

use std::path::{Path, PathBuf};

use kagi_domain::plan_note::{PlanNote, PushNote};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use kagi_git::{Backend, Operation};
use tempfile::TempDir;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

struct Repos {
    _tmp: TempDir,
    local: PathBuf,
    company: PathBuf,
    personal: PathBuf,
    replacement: PathBuf,
}

impl Repos {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let local = tmp.path().join("local");
        std::fs::create_dir(&local).unwrap();
        init_repo(&local, "main");
        write_file(&local, "base.txt", "base\n");
        commit_all(&local, "base");
        let company = tmp.path().join("company.git");
        let personal = tmp.path().join("personal.git");
        let replacement = tmp.path().join("replacement.git");
        for remote in [&company, &personal, &replacement] {
            git(
                tmp.path(),
                &[
                    "init",
                    "-q",
                    "--bare",
                    "-b",
                    "main",
                    remote.to_str().unwrap(),
                ],
            );
        }
        for (name, remote) in [("company", &company), ("personal", &personal)] {
            git(&local, &["remote", "add", name, remote.to_str().unwrap()]);
        }
        // Preserve staged, unstaged and untracked content through refusal.
        write_file(&local, "base.txt", "staged\n");
        git(&local, &["add", "base.txt"]);
        write_file(&local, "base.txt", "unstaged\n");
        write_file(&local, "untracked.txt", "untracked\n");
        Self {
            _tmp: tmp,
            local,
            company,
            personal,
            replacement,
        }
    }

    fn remote_refs(&self) -> Vec<String> {
        [&self.company, &self.personal, &self.replacement]
            .into_iter()
            .map(|remote| {
                git_output(
                    remote,
                    &["for-each-ref", "--format=%(refname) %(objectname)"],
                )
            })
            .collect()
    }

    fn assert_no_upstream(&self, branch: &str) {
        let repo = git2::Repository::open(&self.local).unwrap();
        assert!(repo
            .find_branch(branch, git2::BranchType::Local)
            .unwrap()
            .upstream()
            .is_err());
    }
}

#[derive(Debug, PartialEq, Eq)]
struct LocalState {
    head: String,
    refs: String,
    index: Vec<u8>,
    config: Vec<u8>,
    base: Vec<u8>,
    untracked: Vec<u8>,
}

impl LocalState {
    fn capture(local: &Path) -> Self {
        Self {
            head: git_output(local, &["symbolic-ref", "HEAD"]),
            refs: git_output(
                local,
                &["for-each-ref", "--format=%(refname) %(objectname)"],
            ),
            index: std::fs::read(local.join(".git/index")).unwrap(),
            config: std::fs::read(local.join(".git/config")).unwrap(),
            base: std::fs::read(local.join("base.txt")).unwrap(),
            untracked: std::fs::read(local.join("untracked.txt")).unwrap(),
        }
    }
}

fn operation(sidebar: bool, r: &Repos) -> (Operation, &'static str) {
    if sidebar {
        git(&r.local, &["branch", "publish/sidebar"]);
        (
            Operation::PushBranch {
                branch_name: "publish/sidebar".into(),
                set_upstream: true,
            },
            "publish/sidebar",
        )
    } else {
        (Operation::Push, "main")
    }
}

#[derive(Clone, Copy, Debug)]
enum Drift {
    AddOrigin,
    Url,
    PushUrl,
    RenameCompany,
    ReorderCompany,
    ReplaceCompany,
}

fn inject_drift(r: &Repos, drift: Drift) {
    let replacement = r.replacement.to_str().unwrap();
    match drift {
        Drift::AddOrigin => git(&r.local, &["remote", "add", "origin", replacement]),
        Drift::Url => git(&r.local, &["remote", "set-url", "company", replacement]),
        Drift::PushUrl => git(
            &r.local,
            &["remote", "set-url", "--push", "company", replacement],
        ),
        Drift::RenameCompany => git(
            &r.local,
            &["remote", "rename", "company", "renamed-company"],
        ),
        Drift::ReorderCompany => {
            // Rename changes the implicit first candidate without changing its URL.
            git(&r.local, &["remote", "rename", "company", "zz-company"]);
        }
        Drift::ReplaceCompany => {
            git(&r.local, &["remote", "remove", "company"]);
            git(&r.local, &["remote", "add", "company", replacement]);
        }
    }
}

fn assert_refused_original_plan(sidebar: bool, drift: Drift) {
    let r = Repos::new();
    let (op, branch) = operation(sidebar, &r);
    let mut backend = Backend::open(&r.local).unwrap();
    let approved = backend.plan(&op).unwrap();
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);
    let candidates = approved
        .warnings
        .iter()
        .find_map(|note| match note {
            PlanNote::Push(PushNote::PublishRemoteAmbiguous { candidates }) => Some(candidates),
            _ => None,
        })
        .expect("original no-origin plan must expose typed ambiguity");
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().any(|name| name == "company"));
    assert!(candidates.iter().any(|name| name == "personal"));
    assert_eq!(approved.push_identity.as_ref().unwrap().remote, "company");

    inject_drift(&r, drift);
    // Snapshot after the deliberate config change, not before it.
    let before = LocalState::capture(&r.local);
    let remote_before = r.remote_refs();
    let receipts_before = read_oplog_tail_for_repo(&r.local, 100).len();
    let err = backend
        .run(&op, &approved)
        .expect_err("approved destination drift must refuse");
    assert!(
        matches!(
            err.blocker(),
            Some(PlanNote::Push(PushNote::PushIdentityChanged))
        ),
        "{drift:?}: {err:?}"
    );
    assert_eq!(LocalState::capture(&r.local), before, "{drift:?}");
    assert_eq!(
        r.remote_refs(),
        remote_before,
        "no remote may receive a branch"
    );
    r.assert_no_upstream(branch);
    let receipts = read_oplog_tail_for_repo(&r.local, 100);
    assert_eq!(receipts.len(), receipts_before + 1);
    assert!(
        matches!(&receipts[0].outcome, OpOutcome::Refused { blockers } if !blockers.is_empty())
    );
}

#[test]
fn current_push_original_ambiguous_plan_refuses_added_origin() {
    if !test_support::run_isolated() {
        return;
    }
    assert_refused_original_plan(false, Drift::AddOrigin);
}

#[test]
fn sidebar_publish_original_ambiguous_plan_refuses_added_origin() {
    if !test_support::run_isolated() {
        return;
    }
    assert_refused_original_plan(true, Drift::AddOrigin);
}

#[test]
fn approved_publish_refuses_url_and_pushurl_drift_for_both_producers() {
    if !test_support::run_isolated() {
        return;
    }
    for sidebar in [false, true] {
        for drift in [Drift::Url, Drift::PushUrl] {
            assert_refused_original_plan(sidebar, drift);
        }
    }
}

#[test]
fn approved_publish_refuses_company_rename_order_and_replacement_for_both_producers() {
    if !test_support::run_isolated() {
        return;
    }
    for sidebar in [false, true] {
        for drift in [
            Drift::RenameCompany,
            Drift::ReorderCompany,
            Drift::ReplaceCompany,
        ] {
            assert_refused_original_plan(sidebar, drift);
        }
    }
}

#[test]
fn unchanged_approved_publish_pushes_only_company_once_for_both_producers() {
    if !test_support::run_isolated() {
        return;
    }
    for sidebar in [false, true] {
        let r = Repos::new();
        let (op, branch) = operation(sidebar, &r);
        let mut backend = Backend::open(&r.local).unwrap();
        let approved = backend.plan(&op).unwrap();
        assert!(approved.blockers.is_empty());
        assert_eq!(approved.push_identity.as_ref().unwrap().remote, "company");
        let before = LocalState::capture(&r.local);
        let remote_before = r.remote_refs();
        backend
            .run(&op, &approved)
            .expect("unchanged approved destination succeeds");
        let refname = format!("refs/heads/{branch}");
        assert_eq!(
            git_output(&r.company, &["rev-parse", &refname]),
            git_output(&r.local, &["rev-parse", &refname])
        );
        let after = r.remote_refs();
        assert_eq!(
            &after[1..],
            &remote_before[1..],
            "unrelated remotes must remain untouched"
        );
        let local_after = LocalState::capture(&r.local);
        assert_eq!(local_after.head, before.head);
        assert_eq!(local_after.index, before.index);
        assert_eq!(local_after.base, before.base);
        assert_eq!(local_after.untracked, before.untracked);
        assert_eq!(
            git_output(&r.local, &["config", &format!("branch.{branch}.remote")]),
            "company"
        );
        assert_eq!(
            git_output(&r.local, &["config", &format!("branch.{branch}.merge")]),
            refname
        );
        let receipts = read_oplog_tail_for_repo(&r.local, 100);
        assert_eq!(
            receipts.len(),
            1,
            "one approved run owns one durable receipt"
        );
        assert!(matches!(receipts[0].outcome, OpOutcome::Success { .. }));
    }
}
