#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod test_support;
use git_fixture::{git_output as git, repo_with_bare_origin};
use kagi_git::{Backend, Operation};

#[test]
fn push_tag_authority() {
    if !test_support::run_isolated() {
        return;
    }
    for case in [
        "remote",
        "object",
        "url",
        "annotated_drift",
        "annotated",
        "happy",
        "multi",
        "multi_drift",
    ] {
        let r = repo_with_bare_origin("main");
        let backup = tempfile::TempDir::new().unwrap();
        git(backup.path(), &["init", "--bare", "-q"]);
        git(
            &r.local,
            &["remote", "add", "backup", backup.path().to_str().unwrap()],
        );
        if case.starts_with("multi") {
            git(
                &r.local,
                &[
                    "config",
                    "--add",
                    "remote.origin.pushurl",
                    r.remote.to_str().unwrap(),
                ],
            );
            git(
                &r.local,
                &[
                    "config",
                    "--add",
                    "remote.origin.pushurl",
                    backup.path().to_str().unwrap(),
                ],
            );
        }
        if case.starts_with("annotated") {
            git(
                &r.local,
                &["tag", "-a", "release", "-m", "approved annotation"],
            );
        } else {
            git(&r.local, &["tag", "release"]);
        }
        let approved = git(&r.local, &["rev-parse", "refs/tags/release"]);
        let mut backend = Backend::open(&r.local).unwrap();
        let mut op = Operation::PushTag {
            name: "release".into(),
            remote: "origin".into(),
        };
        let plan = backend.plan(&op).unwrap();
        let identity = plan.tag_push_identity.as_ref().unwrap();
        assert_eq!(
            identity.push_urls.len(),
            if case.starts_with("multi") { 2 } else { 1 }
        );
        assert_eq!(identity.object_oid, approved);
        assert_eq!(
            identity.peeled_oid,
            git(&r.local, &["rev-parse", "refs/tags/release^{}"])
        );
        match case {
            "remote" => {
                op = Operation::PushTag {
                    name: "release".into(),
                    remote: "backup".into(),
                }
            }
            "object" => {
                let repo = git2::Repository::open(&r.local).unwrap();
                let parent = repo.head().unwrap().peel_to_commit().unwrap();
                let signature = repo.signature().unwrap();
                let next = repo
                    .commit(
                        None,
                        &signature,
                        &signature,
                        "unapproved",
                        &parent.tree().unwrap(),
                        &[&parent],
                    )
                    .unwrap();
                repo.reference("refs/tags/release", next, true, "external tag move")
                    .unwrap();
            }
            "annotated_drift" => {
                let repo = git2::Repository::open(&r.local).unwrap();
                let target = repo.head().unwrap().peel_to_commit().unwrap();
                repo.tag(
                    "release",
                    target.as_object(),
                    &repo.signature().unwrap(),
                    "unapproved annotation",
                    true,
                )
                .unwrap();
                assert_eq!(
                    git(&r.local, &["rev-parse", "release^{}"]),
                    identity.peeled_oid
                );
            }
            "url" => {
                git(
                    &r.local,
                    &[
                        "remote",
                        "set-url",
                        "--push",
                        "origin",
                        backup.path().to_str().unwrap(),
                    ],
                );
            }
            "multi_drift" => {
                git(
                    &r.local,
                    &[
                        "config",
                        "--unset-all",
                        "remote.origin.pushurl",
                        backup.path().to_str().unwrap(),
                    ],
                );
            }
            _ => {}
        }
        let receipts_before = kagi_git::oplog::read_oplog_tail(100).len();
        let result = backend.run(&op, &plan);
        let origin = git2::Repository::open_bare(&r.remote).unwrap();
        let backup_repo = git2::Repository::open_bare(backup.path()).unwrap();
        assert_eq!(
            kagi_git::oplog::read_oplog_tail(100).len(),
            receipts_before + 1,
            "{case}: exactly one receipt"
        );
        if matches!(
            case,
            "remote" | "object" | "url" | "annotated_drift" | "multi_drift"
        ) {
            let error = result.expect_err("stale approval must be refused");
            assert_eq!(
                error.blocker(),
                Some(&kagi_domain::plan_note::PlanNote::Tag(
                    kagi_domain::plan_note::TagNote::PushIdentityChanged
                ))
            );
            assert!(
                origin.find_reference("refs/tags/release").is_err(),
                "{case}: origin unchanged"
            );
        } else {
            result.unwrap();
            assert_eq!(
                origin
                    .find_reference("refs/tags/release")
                    .unwrap()
                    .target()
                    .unwrap()
                    .to_string(),
                approved
            );
            if case == "annotated" {
                assert!(origin
                    .find_tag(git2::Oid::from_str(&approved).unwrap())
                    .is_ok());
            }
        }
        if case == "multi" {
            assert_eq!(
                backup_repo
                    .find_reference("refs/tags/release")
                    .unwrap()
                    .target()
                    .unwrap()
                    .to_string(),
                approved
            );
        } else {
            assert!(
                backup_repo.find_reference("refs/tags/release").is_err(),
                "{case}: backup unchanged"
            );
        }
    }
}
