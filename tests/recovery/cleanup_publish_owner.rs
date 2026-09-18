//! #717 round 2: cleanup evidence belongs to the exact published read model.
use crate::evidence_support::{deferred, Reply};
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{build_tab_view, e2e, TabViewState};
use kagi_domain::branch_cleanup::{BranchCleanupRow, MergedBranchStatus};
use kagi_domain::github::{CiState, Mergeable, PullRequest, ReviewState};
use kagi_git::github::PrFetchError;
use kagi_git::{Backend, CommitId};
use std::path::Path;

type ScanResult = Result<
    (
        Vec<BranchCleanupRow>,
        Result<Vec<PullRequest>, PrFetchError>,
    ),
    String,
>;

fn queue_scan(cx: &mut VisualTestAppContext) -> Reply<ScanResult> {
    let (task, reply) = deferred(cx);
    e2e::queue_cleanup_scan(task);
    reply
}

fn view_of(repo: &Path) -> TabViewState {
    let mut backend = Backend::open(repo).expect("open fixture backend");
    let snapshot = backend.snapshot(10_000).expect("snapshot fixture");
    let name = repo
        .file_name()
        .expect("fixture directory name")
        .to_string_lossy();
    build_tab_view(&snapshot, &name)
}

fn row(name: &str, tip: CommitId) -> BranchCleanupRow {
    BranchCleanupRow {
        name: name.into(),
        local_tip: Some(tip),
        remote_tip: None,
        status: MergedBranchStatus::FullyMerged,
        merged_at: Some(1),
        stale: false,
        deletable: true,
        bulk_deletable: true,
    }
}

pub fn pr(number: u64, head: &str) -> PullRequest {
    PullRequest {
        number,
        title: format!("PR {number}"),
        head: head.into(),
        head_sha: format!("{number:0>40}"),
        base: "main".into(),
        ci: CiState::Success,
        review: ReviewState::Approved,
        url: format!("https://github.test/o/r/pull/{number}"),
        author: "tester".into(),
        mergeable: Mergeable::Clean,
        cross_repository: false,
        base_repo: "github.test/o/r".into(),
        ..Default::default()
    }
}

fn row_names(rows: &[BranchCleanupRow]) -> Vec<&str> {
    rows.iter().map(|row| row.name.as_str()).collect()
}

/// A cleanup scan captures A's old model after the pending read key has already
/// been issued. That exact key then accepts a genuinely rebuilt A model while A
/// is in the background, so read revision and scan generation deliberately stay
/// unchanged; only the published-model generation can reject the old evidence.
fn accept_while_background_invalidates_same_revision_scan(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo_a);
    let (owner_a, owner_b) = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b, cx), "open B");
        (app.tabs[0].session, app.tabs[1].session)
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();

    // Keep render from launching a replacement scan after a published view.
    // This changes neither read revision nor either generation under test.
    let scan_rearm_hold = app.update(cx, |_, cx| {
        cx.observe(&app, |app, _, _| app.scans_stale = false)
    });

    let stale = queue_scan(cx);
    let fresh_prs = vec![pr(717, "fresh-deletable")];
    let (pending_key, read_revision, scan_gen, publish_gen, old_rows, old_tip) =
        app.update(cx, |app, cx| {
            assert_eq!(app.active_session(), Some(owner_a));
            app.ui.get_mut(&owner_a).unwrap().cleanup_prs = fresh_prs.clone();
            let pending_key = app.reads.begin(owner_a);
            let read_revision = app.reads.revision(owner_a);
            let old_rows = app
                .reads
                .get(Some(owner_a))
                .rows
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>();
            let old_tip = old_rows.first().expect("old A head").clone();
            app.start_branch_cleanup_scan(cx);
            (
                pending_key,
                read_revision,
                app.ui[&owner_a].cleanup_gen,
                app.ui[&owner_a].view_publish_gen,
                old_rows,
                old_tip,
            )
        });

    std::fs::write(repo_a.join("publish-generation.txt"), "new read model\n").unwrap();
    git(&repo_a, &["add", "publish-generation.txt"]);
    git(
        &repo_a,
        &["commit", "-q", "-m", "advance cleanup publish model"],
    );
    git(&repo_a, &["branch", "fresh-deletable", "HEAD~1"]);
    let mut fresh_view = view_of(&repo_a);
    let fresh_tip = fresh_view.branch_targets["fresh-deletable"].clone();
    fresh_view.cleanup_rows = vec![row("fresh-deletable", fresh_tip)];

    let fresh_rows = app.update(cx, |app, cx| {
        // Depart only after both fixture reads have settled. B's revalidation is
        // issued for B, so it cannot supersede the manual pending key for A.
        app.switch_repo(1, cx);
        assert_eq!(app.active_session(), Some(owner_b));
        assert!(
            app.accept_tab_view(pending_key, fresh_view),
            "the exact pending A key was rejected"
        );
        assert_eq!(
            app.reads.revision(owner_a),
            read_revision,
            "accept advanced the read revision and masked the shared-revision race"
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_gen, scan_gen,
            "background accept advanced the cleanup scan generation"
        );
        assert!(
            app.ui[&owner_a].view_publish_gen > publish_gen,
            "successful background accept did not identify the new read model"
        );
        let fresh_rows = app
            .reads
            .get(Some(owner_a))
            .rows
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>();
        assert_ne!(
            fresh_rows, old_rows,
            "real commit did not change row identities"
        );
        assert_eq!(
            row_names(&app.reads.get(Some(owner_a)).cleanup_rows),
            vec!["fresh-deletable"]
        );
        assert_eq!(app.ui[&owner_a].cleanup_prs, fresh_prs);
        fresh_rows
    });

    stale.send(Ok((
        vec![row("obsolete-deletable", old_tip)],
        Ok(vec![pr(718, "obsolete-deletable")]),
    )));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(app.active_session(), Some(owner_b));
        assert_eq!(
            app.reads.revision(owner_a),
            read_revision,
            "an incidental A read masked the same-revision settlement oracle"
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_gen, scan_gen,
            "a replacement scan masked the published-model settlement oracle"
        );
        assert_eq!(
            app.reads
                .get(Some(owner_a))
                .rows
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>(),
            fresh_rows,
            "stale cleanup completion replaced the accepted commit rows"
        );
        assert_eq!(
            row_names(&app.reads.get(Some(owner_a)).cleanup_rows),
            vec!["fresh-deletable"],
            "stale cleanup completion overwrote fresh cleanup rows"
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_prs, fresh_prs,
            "stale cleanup completion overwrote fresh PR evidence"
        );
        assert!(!app.ui[&owner_a].cleanup_scanning);
    });

    // Inspect the real selection and plan consumers synchronously: switch_repo
    // has issued its automatic read, but that read cannot accept until control
    // returns to the dispatcher.
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert_eq!(row_names(&app.view().cleanup_rows), vec!["fresh-deletable"]);
        assert!(!app
            .view()
            .cleanup_rows
            .iter()
            .any(|row| row.name == "obsolete-deletable"));
        app.toggle_cleanup_select_all(cx);
        assert_eq!(
            app.ui()
                .cleanup_selected
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["fresh-deletable"],
            "select-all consumed obsolete cleanup evidence"
        );
        app.delete_selected_cleanup_branches(cx);
        let targets = &app
            .branch_cleanup_modal()
            .expect("fresh cleanup delete plan")
            .targets;
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name, "fresh-deletable");
        app.cancel_branch_cleanup_modal();
    });
    cx.run_until_parked();

    let fresh = queue_scan(cx);
    let current_tip = app.update(cx, |app, cx| {
        let current_tip = app.view().rows[0].id.clone();
        app.start_branch_cleanup_scan(cx);
        current_tip
    });
    fresh.send(Ok((
        vec![row("current-deletable", current_tip)],
        Ok(vec![pr(719, "current-deletable")]),
    )));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            row_names(&app.view().cleanup_rows),
            vec!["current-deletable"]
        );
        assert_eq!(app.ui().cleanup_prs, vec![pr(719, "current-deletable")]);
        assert!(!app.ui().cleanup_scanning);
    });

    drop(scan_rearm_hold);
    unmount(cx, app, window);
}

/// Exercise the other public publish boundaries through cleanup settlement:
/// owner publish/amend invalidate captured evidence, while a rejected accept or
/// another owner's publish does not invalidate an otherwise-current A scan.
fn publish_boundaries_invalidate_only_their_owner(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo_a);
    let (owner_a, owner_b) = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        (app.tabs[0].session, app.tabs[1].session)
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    let scan_rearm_hold = app.update(cx, |_, cx| {
        cx.observe(&app, |app, _, _| app.scans_stale = false)
    });
    let tip = app.update(cx, |app, _| app.view().rows[0].id.clone());

    // Publishing B must not invalidate evidence captured for A.
    let other_owner = queue_scan(cx);
    app.update(cx, |app, cx| app.start_branch_cleanup_scan(cx));
    app.update(cx, |app, _| app.publish_tab_view(owner_b, view_of(&repo_b)));
    other_owner.send(Ok((
        vec![row("after-other-owner-publish", tip.clone())],
        Ok(vec![pr(720, "after-other-owner-publish")]),
    )));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            row_names(&app.reads.get(Some(owner_a)).cleanup_rows),
            vec!["after-other-owner-publish"]
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_prs,
            vec![pr(720, "after-other-owner-publish")]
        );
    });

    // A rejected accept is not a publication and must leave an A scan valid.
    let rejected_accept = queue_scan(cx);
    let (rejected, live) = app.update(cx, |app, cx| {
        let rejected = app.reads.begin(owner_a);
        let live = app.reads.begin(owner_a);
        app.start_branch_cleanup_scan(cx);
        (rejected, live)
    });
    let rejected_view = view_of(&repo_a);
    app.update(cx, |app, _| {
        assert!(
            !app.accept_tab_view(rejected, rejected_view),
            "superseded accept unexpectedly published"
        );
    });
    rejected_accept.send(Ok((
        vec![row("after-rejected-accept", tip.clone())],
        Ok(vec![pr(721, "after-rejected-accept")]),
    )));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            row_names(&app.reads.get(Some(owner_a)).cleanup_rows),
            vec!["after-rejected-accept"]
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_prs,
            vec![pr(721, "after-rejected-accept")]
        );
        assert!(app.reads.fail(live), "settle live manual read");
    });

    // Amend replaces A's model without advancing its read revision. The held
    // evidence therefore depends specifically on the publish-generation guard.
    let amended_stale = queue_scan(cx);
    let (amend_revision, amend_gen) = app.update(cx, |app, cx| {
        app.start_branch_cleanup_scan(cx);
        (app.reads.revision(owner_a), app.ui[&owner_a].cleanup_gen)
    });
    let mut amended = view_of(&repo_a);
    amended.cleanup_rows = vec![row("amend-fresh", tip.clone())];
    app.update(cx, |app, _| app.amend_tab_view(owner_a, amended));
    amended_stale.send(Ok((
        vec![row("amend-obsolete", tip.clone())],
        Ok(vec![pr(722, "amend-obsolete")]),
    )));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(app.reads.revision(owner_a), amend_revision);
        assert_eq!(app.ui[&owner_a].cleanup_gen, amend_gen);
        assert_eq!(
            row_names(&app.reads.get(Some(owner_a)).cleanup_rows),
            vec!["amend-fresh"],
            "stale cleanup completion overwrote amended cleanup rows"
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_prs,
            vec![pr(721, "after-rejected-accept")]
        );
    });

    // A full public publish also invalidates captured evidence. Its read-key
    // change is expected, but the observable contract is still that the new
    // model survives the old callback.
    let published_stale = queue_scan(cx);
    app.update(cx, |app, cx| app.start_branch_cleanup_scan(cx));
    let mut published = view_of(&repo_a);
    published.cleanup_rows = vec![row("publish-fresh", tip.clone())];
    app.update(cx, |app, _| app.publish_tab_view(owner_a, published));
    published_stale.send(Ok((
        vec![row("publish-obsolete", tip)],
        Ok(vec![pr(723, "publish-obsolete")]),
    )));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            row_names(&app.reads.get(Some(owner_a)).cleanup_rows),
            vec!["publish-fresh"],
            "stale cleanup completion overwrote publicly published cleanup rows"
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_prs,
            vec![pr(721, "after-rejected-accept")]
        );
    });

    drop(scan_rearm_hold);
    unmount(cx, app, window);
}

pub fn scenario_cleanup_evidence_publish_generation(cx: &mut VisualTestAppContext) {
    accept_while_background_invalidates_same_revision_scan(cx);
    publish_boundaries_invalidate_only_their_owner(cx);
}
