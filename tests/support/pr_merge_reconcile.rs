//! Fork merge reconciliation checks the approved local deletion, not a base ref.
use super::*;
/// #705: a fork PR's head branch is not in the base repository, so
/// `refs/heads/<head>` there is absent from the start — and `RemoteExpect::Absent`
/// would happily call that "deleted" (#701 final review 2). Upstream `gh`
/// skips remote deletion for a cross-repository PR. Kagi pins `-R` and owns
/// the guarded local deletion. That local branch is the only deletion
/// this merge promises, and the only one the reconcile read may confirm: the
/// fork's own branch is not kagi's to touch, and a base-repository ref that
/// was never there may not stand in for anything.
///
/// The merge below ends `Unknown` — `gh` exited, the server could not be
/// read — so nothing local was touched, and a later "yes, merged" alone does
/// not finish it while the branch is still sitting there.
#[test]
fn pr_merge_from_a_fork_promises_only_the_local_branch() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    let bin = dir.join("fake-bin");
    // The fork's head branch, fetched locally to review it.
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
    assert_eq!(
        plan.disposition,
        kagi_domain::plan_note::PlanDisposition::Ready,
        "the local half is kagi's to do, so the option is offerable: {:?}",
        plan.blockers
    );
    assert!(
        plan.warnings.iter().any(|note| matches!(
            note,
            kagi_domain::plan_note::PlanNote::Github(
                kagi_domain::plan_note::GithubNote::ForkKeepsRemoteBranch
            )
        )),
        "the user is told the fork keeps its branch: {:?}",
        plan.warnings
    );
    assert!(
        plan.warnings.iter().any(|note| matches!(
            note,
            kagi_domain::plan_note::PlanNote::Github(
                kagi_domain::plan_note::GithubNote::DeletesLocalBranch { branch, tip: frozen }
            ) if branch == HEAD_BRANCH && frozen.as_deref() == Some(tip.as_str())
        )),
        "and what is actually deleted is named with its exact tip: {:?}",
        plan.warnings
    );
    let repo_id = backend.write_repo_id().unwrap();
    let remote = backend.remote_expectation("pr-merge", &plan);
    drop(backend);
    assert!(
        remote.contains(
            &kagi_git::backend::remote_ref::RemoteExpectation::PullRequest {
                base_repo: BASE_REPO.to_string(),
                number: 501,
                expect: kagi_git::backend::remote_ref::PrExpect::Merged,
            }
        ),
        "the merge itself is still promised: {remote:?}"
    );
    assert!(
        !remote.iter().any(|expectation| matches!(
            expectation,
            kagi_git::backend::remote_ref::RemoteExpectation::GithubRef { .. }
        )),
        "no ref that was never in the base repository may stand in for a deletion: {remote:?}"
    );
    let local = remote
        .iter()
        .find_map(|expectation| match expectation {
            kagi_git::backend::remote_ref::RemoteExpectation::LocalBranch { branch } => {
                Some(branch)
            }
            _ => None,
        })
        .expect("the local branch is the deletion a fork merge promises");
    assert_eq!(
        (local.name.as_str(), local.tip.as_deref()),
        (HEAD_BRANCH, Some(tip.as_str()))
    );

    let mut sessions = kagi::app::Sessions::new();
    let session = sessions.attach(dir.clone());
    let owner = sessions.attachment(session).unwrap();
    let approved = kagi::app::approve_run(
        &mut sessions,
        kagi::app::RunRequest {
            owner,
            name: "pr-merge",
            path: dir.clone(),
            repo: repo_id,
            plan: std::sync::Arc::new(plan.clone()),
            remote,
        },
    )
    .unwrap();
    // `gh` exited and the server could not be re-read: the merge is neither
    // confirmed nor refuted, so the local branch was left alone.
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
    assert_eq!(
        git(dir, &["rev-parse", HEAD_BRANCH]).trim(),
        tip,
        "an unconfirmed merge deletes nothing locally"
    );

    // The server says merged — and the branch the promise named is still
    // here, so the operation is not finished.
    {
        let _gh = FakeGh::answering(&bin, r#"echo '{"mergedAt":"2026-09-07T00:00:00Z"}'"#);
        let read = kagi::app::read_reconcile(&sessions, id).unwrap();
        assert!(
            !read.resolved(),
            "merged is only half of what was promised: {}",
            read.observation
        );
        assert!(
            read.observation.contains(HEAD_BRANCH),
            "and the observation says which branch is still there: {}",
            read.observation
        );
        assert!(kagi::app::acknowledge(&mut sessions, read).is_err());
    }
    // The branch goes for real — by whatever hand — and only then is it done.
    git(dir, &["branch", "-D", HEAD_BRANCH]);
    {
        let _gh = FakeGh::answering(&bin, r#"echo '{"mergedAt":"2026-09-07T00:00:00Z"}'"#);
        let read = kagi::app::read_reconcile(&sessions, id).unwrap();
        assert!(
            read.resolved(),
            "merged, and the branch it promised is gone: {}",
            read.observation
        );
        kagi::app::acknowledge(&mut sessions, read).expect("a whole kept promise closes it");
    }
    assert!(sessions.reconcile_ids().is_empty());

    // Same PR without the option: an ordinary merge promises the merge alone.
    let plain = Backend::open(dir)
        .unwrap()
        .plan_pr_merge(
            &one_pr(true),
            kagi_git::github::MergeMethod::Squash,
            false,
            "branch 'main'".into(),
        )
        .unwrap();
    assert_eq!(
        plain.disposition,
        kagi_domain::plan_note::PlanDisposition::Ready
    );
    assert_eq!(
        Backend::open(dir)
            .unwrap()
            .remote_expectation("pr-merge", &plain),
        vec![
            kagi_git::backend::remote_ref::RemoteExpectation::PullRequest {
                base_repo: BASE_REPO.to_string(),
                number: 501,
                expect: kagi_git::backend::remote_ref::PrExpect::Merged,
            }
        ],
        "a merge that deletes nothing promises nothing to delete"
    );
}
