//! A merge Unknown must not require deletion of a local branch it never touched.
use super::*;

#[test]
fn fork_merge_reconciliation_releases_untouched_local_branch_for_guarded_cleanup() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    let bin = dir.join("fake-bin");
    git(dir, &["branch", HEAD_BRANCH]);
    let tip = git(dir, &["rev-parse", HEAD_BRANCH]).trim().to_string();
    let mut pr = one_pr(true);
    pr.head_sha = tip.clone();
    let backend = Backend::open(dir).unwrap();
    let plan = backend
        .plan_pr_merge(
            &pr,
            kagi_git::github::MergeMethod::Squash,
            true,
            "branch 'main'".into(),
        )
        .unwrap();
    let repo_id = backend.write_repo_id().unwrap();
    let remote = backend.remote_expectation("pr-merge", &plan);
    drop(backend);
    let mut sessions = kagi::app::Sessions::new();
    let session = sessions.attach(dir.clone());
    let owner = sessions.attachment(session).unwrap();
    let approved = kagi::app::approve_run(
        &mut sessions,
        kagi::app::RunRequest {
            owner: owner.clone(),
            name: "pr-merge",
            path: dir.clone(),
            repo: repo_id.clone(),
            plan: std::sync::Arc::new(plan.clone()),
            remote,
        },
    )
    .unwrap();
    let merge_dir = dir.clone();
    let merge_head = tip.clone();
    let _failed_gh = FakeGh::answering(&bin, "echo offline >&2; exit 1");
    let job = kagi::app::prepare_run(
        &mut sessions,
        approved,
        Box::new(move || {
            Ok(kagi_git::github::merge_pr(
                &merge_dir,
                501,
                kagi_git::github::MergeMethod::Squash,
                true,
                &merge_head,
                &plan,
            ))
        }),
    )
    .unwrap();
    let id = job.id();
    sessions.apply(job.run());
    assert_eq!(sessions.reconcile_ids(), vec![id]);
    {
        let _gh = FakeGh::answering(&bin, r#"echo '{"mergedAt":null}'"#);
        let read = kagi::app::read_reconcile(&sessions, id).unwrap();
        assert!(!read.resolved());
        assert!(kagi::app::acknowledge(&mut sessions, read).is_err());
    }
    {
        let _gh = FakeGh::answering(&bin, r#"echo '{"mergedAt":"2026-09-07T00:00:00Z"}'"#);
        let read = kagi::app::read_reconcile(&sessions, id).unwrap();
        assert!(
            read.resolved(),
            "local cleanup never started: {}",
            read.observation
        );
        kagi::app::acknowledge(&mut sessions, read).unwrap();
    }
    assert_eq!(git(dir, &["rev-parse", HEAD_BRANCH]).trim(), tip);
    assert!(sessions.reconcile_ids().is_empty());
    // The user can now use Kagi's ordinary guarded deletion, not an external
    // force-delete escape hatch. Its mandatory backup ref retains the tip.
    let backend = Backend::open(dir).unwrap();
    let delete_plan = std::sync::Arc::new(backend.plan_delete_branch(HEAD_BRANCH).unwrap());
    drop(backend);
    let approved = kagi::app::approve_run(
        &mut sessions,
        kagi::app::RunRequest {
            owner,
            name: "delete-branch",
            path: dir.clone(),
            repo: repo_id,
            plan: delete_plan.clone(),
            remote: vec![],
        },
    )
    .unwrap();
    let delete_dir = dir.clone();
    let job = kagi::app::prepare_run(
        &mut sessions,
        approved,
        Box::new(move || {
            let mut backend = Backend::open(&delete_dir).map_err(|error| error.to_string())?;
            let report = backend.run_recorded(
                &kagi_git::Operation::DeleteBranch {
                    name: HEAD_BRANCH.into(),
                },
                &delete_plan,
            );
            assert!(report.result.is_ok(), "{:?}", report.result);
            let backups = &report.recording.entry().backup_refs;
            assert_eq!(backups.len(), 1);
            assert_eq!(git(&delete_dir, &["rev-parse", &backups[0]]).trim(), tip);
            Ok(report)
        }),
    )
    .unwrap();
    sessions.apply(job.run());
    assert!(git(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname)",
            &format!("refs/heads/{HEAD_BRANCH}")
        ]
    )
    .is_empty());
    assert!(!sessions.has_leases());
}
