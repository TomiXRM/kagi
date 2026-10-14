//! #1560: publication preview is the same frozen identity execution approves.
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

use git_fixture::{git_output as git, repo_with_bare_origin};
use kagi_git::{Backend, Operation};

#[test]
fn push_tag_scope_freezes_non_head_object_kind_and_push_destinations() {
    if !test_support::run_isolated() {
        return;
    }
    for annotated in [false, true] {
        let fixture = repo_with_bare_origin("main");
        let destination = tempfile::TempDir::new().unwrap();
        git(destination.path(), &["init", "--bare", "-q"]);
        git(
            &fixture.local,
            &[
                "config",
                "remote.origin.pushurl",
                destination.path().to_str().unwrap(),
            ],
        );
        let target = git(&fixture.local, &["rev-parse", "HEAD"]);
        if annotated {
            git(
                &fixture.local,
                &["tag", "-a", "release", "-m", "approved annotation"],
            );
        } else {
            git(&fixture.local, &["tag", "release"]);
        }
        std::fs::write(fixture.local.join("later"), "unrelated active HEAD\n").unwrap();
        git(&fixture.local, &["add", "."]);
        git(&fixture.local, &["commit", "-qm", "unrelated active HEAD"]);
        let head = git(&fixture.local, &["rev-parse", "HEAD"]);
        let object = git(&fixture.local, &["rev-parse", "refs/tags/release"]);
        let mut backend = Backend::open(&fixture.local).unwrap();
        let operation = Operation::PushTag {
            name: "release".into(),
            remote: "origin".into(),
        };
        let plan = backend.plan(&operation).unwrap();
        let identity = plan.tag_push_identity.as_ref().unwrap();
        assert_eq!(identity.object_oid, object);
        assert_eq!(identity.peeled_oid, target);
        assert_ne!(identity.peeled_oid, head);
        assert_eq!(identity.push_urls, [destination.path().to_str().unwrap()]);
        assert_eq!(
            identity.kind,
            if annotated {
                kagi_domain::plan::TagKind::Annotated
            } else {
                kagi_domain::plan::TagKind::Lightweight
            }
        );
        assert_eq!(plan.current.head, "branch: main");
        assert_eq!(plan.predicted.head, "branch: main");
        assert_eq!(plan.current.dirty, "clean");
        assert_eq!(plan.predicted.dirty, "clean");
        backend.run(&operation, &plan).unwrap();
        assert_eq!(
            git(destination.path(), &["rev-parse", "refs/tags/release"]),
            object
        );
        let fetch_remote = git2::Repository::open_bare(&fixture.remote).unwrap();
        assert!(fetch_remote.find_reference("refs/tags/release").is_err());
        assert_eq!(git(&fixture.local, &["rev-parse", "HEAD"]), head);
        let receipts = kagi_git::oplog::read_oplog_tail_for_repo(&fixture.local, 100);
        let entry = receipts
            .iter()
            .find(|entry| entry.op == "push-tag")
            .unwrap();
        let kagi_git::oplog::OpOutcome::Success { after } = &entry.outcome else {
            panic!("expected successful receipt: {:?}", entry.outcome);
        };
        assert_eq!(after.head, "branch: main");
        assert_eq!(after.dirty, "clean");
    }
}
