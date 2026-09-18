//! #643 S2b: cleanup scans settle against their frozen session and request token.
use crate::evidence_support::{deferred, Reply};
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::e2e;
use kagi_domain::branch_cleanup::{BranchCleanupRow, MergedBranchStatus};
use kagi_domain::github::{CiState, Mergeable, PullRequest, ReviewState};
use kagi_git::github::PrFetchError;
use kagi_git::CommitId;

// The seam replaces only this transport result; start_branch_cleanup_scan still
// captures the owner, advances its token, and performs production settlement.
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

fn row(name: &str) -> BranchCleanupRow {
    BranchCleanupRow {
        name: name.into(),
        local_tip: Some(CommitId(format!("{name:0<40}"))),
        remote_tip: None,
        status: MergedBranchStatus::FullyMerged,
        merged_at: Some(1),
        stale: false,
        deletable: true,
        bulk_deletable: true,
    }
}

fn pr(number: u64, head: &str) -> PullRequest {
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

pub fn scenario_cleanup_evidence_background_owner(cx: &mut VisualTestAppContext) {
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

    // Settle A's switch-triggered publication before capturing scan evidence;
    // this scenario isolates background-owner routing, not model replacement.
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();

    let pending = queue_scan(cx);
    app.update(cx, |app, cx| {
        assert_eq!(app.active_session(), Some(owner_a));
        app.ui.get_mut(&owner_a).unwrap().cleanup_prs = vec![pr(10, "a-old")];
        app.start_branch_cleanup_scan(cx);
        app.switch_repo(1, cx);
    });
    // Settle B's own switch-triggered read/scan before marking its display.
    // A's injected future stays pending while the dispatcher drains.
    cx.run_until_parked();
    app.update(cx, |app, _| {
        app.reads.get_mut(Some(owner_b)).cleanup_rows = vec![row("b-row")];
        let ui_b = app.ui.get_mut(&owner_b).unwrap();
        ui_b.cleanup_prs = vec![pr(20, "b-row")];
        ui_b.cleanup_prs_stale = false;
        ui_b.cleanup_selected.insert("b-selected".into());
    });

    pending.send(Ok((
        vec![row("a-fresh")],
        Err(PrFetchError::Network("offline".into())),
    )));
    cx.run_until_parked();

    app.update(cx, |app, _| {
        assert_eq!(
            row_names(&app.reads.get(Some(owner_b)).cleanup_rows),
            vec!["b-row"],
            "A's background cleanup rows overwrote B's read",
        );
        assert_eq!(
            app.ui[&owner_b].cleanup_prs,
            vec![pr(20, "b-row")],
            "A's background PR evidence overwrote B's evidence",
        );
        assert!(
            app.ui().cleanup_selected.contains("b-selected"),
            "A's background cleanup completion pruned B's selection",
        );
        assert_eq!(
            row_names(&app.reads.get(Some(owner_a)).cleanup_rows),
            vec!["a-fresh"],
            "the background scan did not update its frozen owner's rows",
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_prs,
            vec![pr(10, "a-old")],
            "a failed PR fetch discarded the owner's last good evidence",
        );
        assert!(
            app.ui[&owner_a].cleanup_prs_stale,
            "a failed PR fetch did not mark the owner's evidence stale",
        );
        assert!(
            !app.ui[&owner_a].cleanup_scanning,
            "the accepted owner scan remained marked as scanning",
        );
    });

    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert_eq!(
            row_names(&app.view().cleanup_rows),
            vec!["a-fresh"],
            "returning to A did not restore A's cleanup rows",
        );
        assert!(
            app.ui().cleanup_prs_stale,
            "returning to A did not restore A's stale-evidence warning",
        );
    });
    unmount(cx, app, window);
}

pub fn scenario_cleanup_evidence_superseded(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    cx.run_until_parked();
    let owner = app.update(cx, |app, _| {
        let owner = app.active_session().unwrap();
        app.reads.get_mut(Some(owner)).cleanup_rows = vec![row("baseline")];
        app.ui.get_mut(&owner).unwrap().cleanup_prs = vec![pr(1, "baseline")];
        owner
    });

    // Older finishes first: it must neither publish nor clear the newer spinner.
    let older = queue_scan(cx);
    app.update(cx, |app, cx| app.start_branch_cleanup_scan(cx));
    let newer = queue_scan(cx);
    app.update(cx, |app, cx| app.start_branch_cleanup_scan(cx));
    older.send(Ok((
        vec![row("older-first")],
        Ok(vec![pr(2, "older-first")]),
    )));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert!(
            app.ui[&owner].cleanup_scanning,
            "an older completion cleared the newer scan's spinner",
        );
        assert_eq!(
            row_names(&app.reads.get(Some(owner)).cleanup_rows),
            vec!["baseline"],
            "an older completion published rows before the newer scan",
        );
        assert_eq!(
            app.ui[&owner].cleanup_prs,
            vec![pr(1, "baseline")],
            "an older completion published PR evidence before the newer scan",
        );
    });
    newer.send(Ok((
        vec![row("newer-first")],
        Ok(vec![pr(3, "newer-first")]),
    )));
    cx.run_until_parked();

    // Older finishes last: the already accepted newer result remains authoritative.
    let older = queue_scan(cx);
    app.update(cx, |app, cx| app.start_branch_cleanup_scan(cx));
    let newer = queue_scan(cx);
    app.update(cx, |app, cx| app.start_branch_cleanup_scan(cx));
    newer.send(Ok((vec![row("newest")], Ok(vec![pr(5, "newest")]))));
    cx.run_until_parked();
    older.send(Ok((vec![row("older-last")], Ok(vec![pr(4, "older-last")]))));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert!(
            !app.ui[&owner].cleanup_scanning,
            "the accepted newest scan did not clear its spinner",
        );
        assert_eq!(
            row_names(&app.reads.get(Some(owner)).cleanup_rows),
            vec!["newest"],
            "an older late completion overwrote the newest cleanup rows",
        );
        assert_eq!(
            app.ui[&owner].cleanup_prs,
            vec![pr(5, "newest")],
            "an older late completion overwrote the newest PR evidence",
        );
        assert!(
            !app.ui[&owner].cleanup_prs_stale,
            "successful newest PR evidence remained marked stale",
        );
    });
    unmount(cx, app, window);
}

pub fn scenario_cleanup_evidence_read_revision(cx: &mut VisualTestAppContext) {
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

    let stale = queue_scan(cx);
    let evidence = vec![pr(70, "last-good")];
    let (scan_revision, scan_gen, old_read_rows) = app.update(cx, |app, cx| {
        app.ui.get_mut(&owner_a).unwrap().cleanup_prs = evidence.clone();
        app.start_branch_cleanup_scan(cx);
        (
            app.reads.revision(owner_a),
            app.ui[&owner_a].cleanup_gen,
            app.reads
                .get(Some(owner_a))
                .rows
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>(),
        )
    });

    std::fs::write(repo_a.join("read-revision.txt"), "new read\n").unwrap();
    git(&repo_a, &["add", "read-revision.txt"]);
    git(&repo_a, &["commit", "-q", "-m", "advance cleanup read"]);
    let launched_revision = app.update(cx, |app, cx| {
        app.reload_async(false, cx);
        let revision = app.reads.revision(owner_a);
        assert!(
            revision > scan_revision,
            "reload_async did not advance A's read revision"
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_gen, scan_gen,
            "background read incorrectly advanced the cleanup generation"
        );
        app.switch_repo(1, cx);
        assert_eq!(
            app.active_session(),
            Some(owner_b),
            "A must depart before accept"
        );
        revision
    });
    cx.run_until_parked();

    let (fresh_read_rows, fresh_cleanup_rows) = app.update(cx, |app, _| {
        assert_eq!(
            app.active_session(),
            Some(owner_b),
            "A's new read did not accept in the background"
        );
        assert_eq!(
            app.reads.revision(owner_a),
            launched_revision,
            "the accepted A read is not the launched revision"
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_gen, scan_gen,
            "background acceptance hid the missing revision guard behind a new generation"
        );
        let fresh_read_rows = app
            .reads
            .get(Some(owner_a))
            .rows
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>();
        assert_ne!(
            fresh_read_rows, old_read_rows,
            "real commit did not replace A's read rows"
        );
        assert!(
            app.reads
                .get(Some(owner_a))
                .rows
                .iter()
                .any(|row| row.summary == "advance cleanup read"),
            "A's accepted background read does not contain the real commit"
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_prs, evidence,
            "accepting the read changed last-good cleanup PR evidence"
        );
        assert!(
            app.ui[&owner_a].cleanup_scanning,
            "A's held scan did not remain in flight"
        );
        (
            fresh_read_rows,
            row_names(&app.reads.get(Some(owner_a)).cleanup_rows)
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        )
    });

    stale.send(Ok((
        vec![row("obsolete-deletable")],
        Ok(vec![pr(71, "obsolete-deletable")]),
    )));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            app.active_session(),
            Some(owner_b),
            "stale completion changed the active owner"
        );
        assert_eq!(
            app.reads
                .get(Some(owner_a))
                .rows
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>(),
            fresh_read_rows,
            "stale cleanup completion replaced the accepted read rows"
        );
        assert_eq!(
            row_names(&app.reads.get(Some(owner_a)).cleanup_rows),
            fresh_cleanup_rows
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            "stale cleanup completion overwrote fresh cleanup rows"
        );
        assert_eq!(
            app.ui[&owner_a].cleanup_prs, evidence,
            "stale cleanup completion overwrote last-good PR evidence"
        );
        assert!(
            !app.ui[&owner_a].cleanup_scanning,
            "the stale request did not settle its own spinner"
        );
    });

    // Inspect the real selection and delete consumers before the switch-triggered
    // refresh gets an executor turn to replace the read again.
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert_eq!(
            app.reads
                .get(Some(owner_a))
                .rows
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>(),
            fresh_read_rows,
            "returning to A did not expose the accepted read"
        );
        assert!(
            !app.view()
                .cleanup_rows
                .iter()
                .any(|row| row.name == "obsolete-deletable"),
            "obsolete deletable candidate became visible after returning to A"
        );
        app.toggle_cleanup_select_all(cx);
        assert!(
            app.ui().cleanup_selected.is_empty(),
            "obsolete candidate was available to cleanup selection"
        );
        app.delete_selected_cleanup_branches(cx);
        assert!(
            app.branch_cleanup_modal().is_none(),
            "obsolete candidate reached the delete-plan consumer"
        );
    });
    cx.run_until_parked();

    let fresh = queue_scan(cx);
    app.update(cx, |app, cx| app.start_branch_cleanup_scan(cx));
    fresh.send(Ok((
        vec![row("current-deletable")],
        Ok(vec![pr(72, "current-deletable")]),
    )));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            row_names(&app.view().cleanup_rows),
            vec!["current-deletable"],
            "a scan for the current read revision did not settle"
        );
        assert_eq!(
            app.ui().cleanup_prs,
            vec![pr(72, "current-deletable")],
            "fresh cleanup PR evidence did not settle"
        );
        assert!(
            !app.ui().cleanup_scanning,
            "fresh cleanup scan remained marked in flight"
        );
    });
    unmount(cx, app, window);
}
