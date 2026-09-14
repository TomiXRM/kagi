//! #717: squash evidence settles only against the read model generation it scanned.
use crate::evidence_support::{deferred, Reply};
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{build_tab_view, e2e, graph_squash::GHOST_COLOR, KagiApp, TabViewState};
use kagi_git::ops::SquashLink;
use kagi_git::{Backend, CommitId};
use std::path::Path;

type ScanResult = Result<Vec<SquashLink>, kagi_git::GitError>;
type GraphSignature = Vec<(String, usize, usize, Vec<(usize, usize, String, usize)>)>;

fn queue_scan(cx: &mut VisualTestAppContext) -> Reply<ScanResult> {
    let (task, reply) = deferred(cx);
    e2e::queue_squash_scan(task);
    reply
}

fn link(tip: &CommitId, squash: &CommitId, branch: &str) -> SquashLink {
    SquashLink {
        branch: branch.into(),
        tip: tip.0.clone(),
        squash: squash.0.clone(),
    }
}

fn build_view(repo: &Path) -> TabViewState {
    let mut backend = Backend::open(repo).expect("open fixture backend");
    let snapshot = backend.snapshot(1_000).expect("snapshot fixture");
    let name = repo
        .file_name()
        .and_then(|name| name.to_str())
        .expect("fixture repository name");
    build_tab_view(&snapshot, name)
}

fn graph_signature(app: &KagiApp, owner: kagi::app::SessionId) -> GraphSignature {
    app.reads
        .get(Some(owner))
        .rows
        .iter()
        .map(|row| {
            (
                row.id.0.clone(),
                row.lane,
                row.lane_count,
                row.edges
                    .iter()
                    .map(|edge| {
                        (
                            edge.from_lane,
                            edge.to_lane,
                            format!("{:?}", edge.kind),
                            edge.color,
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

fn ghost_rows(app: &KagiApp, owner: kagi::app::SessionId) -> Vec<String> {
    app.reads
        .get(Some(owner))
        .rows
        .iter()
        .filter(|row| row.edges.iter().any(|edge| edge.color == GHOST_COLOR))
        .map(|row| row.id.0.clone())
        .collect()
}

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

    // Read-completion and tab-switch notifications would normally re-arm a
    // replacement scan in render. Hold only that scheduling flag so the held
    // completion is decided by the captured read/publish identity.
    let scan_rearm_hold = app.update(cx, |_, cx| {
        cx.observe(&app, |app, _, _| app.scans_stale = false)
    });

    let stale = queue_scan(cx);
    let (read_key, scan_revision, scan_gen, scan_publish_gen, old_head, old_root) =
        app.update(cx, |app, cx| {
            let read_key = app.reads.begin(owner_a);
            let scan_revision = app.reads.revision(owner_a);
            let rows = &app.reads.get(Some(owner_a)).rows;
            let old_head = rows.first().expect("A head row").id.clone();
            let old_root = rows.last().expect("A root row").id.clone();
            app.start_squash_link_scan(cx);
            let scan_gen = app.squash_gen;
            let scan_publish_gen = app.ui[&owner_a].view_publish_gen;
            app.switch_repo(1, cx);
            (
                read_key,
                scan_revision,
                scan_gen,
                scan_publish_gen,
                old_head,
                old_root,
            )
        });

    std::fs::write(repo_a.join("publish-generation.txt"), "new model\n").unwrap();
    git(&repo_a, &["add", "publish-generation.txt"]);
    git(
        &repo_a,
        &["commit", "-q", "-m", "advance squash published model"],
    );
    let new_view = build_view(&repo_a);
    let new_head = new_view.rows[0].id.clone();
    assert_ne!(new_head, old_head, "real commit did not rebuild the graph");

    app.update(cx, |app, _| {
        assert_eq!(app.active_session(), Some(owner_b), "A must be background");
        assert!(
            app.accept_tab_view(read_key, new_view),
            "same-revision A read was not accepted"
        );
    });
    cx.run_until_parked();

    let accepted_signature = app.update(cx, |app, _| {
        assert_eq!(
            app.reads.revision(owner_a),
            scan_revision,
            "accept advanced the read revision"
        );
        assert_eq!(
            app.squash_gen, scan_gen,
            "a replacement scan masked the publish-generation guard"
        );
        assert!(
            app.ui[&owner_a].view_publish_gen > scan_publish_gen,
            "background accept did not advance A's published-model generation"
        );
        assert_eq!(
            app.reads.get(Some(owner_a)).rows[0].id,
            new_head,
            "background accept did not publish the real commit"
        );
        assert!(
            ghost_rows(app, owner_a).is_empty(),
            "accepted graph already contained squash evidence"
        );
        graph_signature(app, owner_a)
    });

    // A real switch starts a newer read immediately, which would make this a
    // read-revision test. Move only the active ownership/path scheduling seam
    // back to A; no read revision, scan generation, or publish generation is
    // assigned. This lets the production scan callback exercise its remaining
    // owner and same-request guards against the newly accepted model.
    app.update(cx, |app, _| {
        app.active_tab = 0;
        app.repo_path = Some(repo_a.clone());
        assert_eq!(
            app.active_session(),
            Some(owner_a),
            "A is active for settlement"
        );
        assert_eq!(
            app.reads.revision(owner_a),
            scan_revision,
            "scheduling seam changed the read revision"
        );
        assert_eq!(
            app.squash_gen, scan_gen,
            "scheduling seam changed the squash generation"
        );
    });

    stale.send(Ok(vec![link(
        &old_root,
        &old_head,
        "stale-published-model",
    )]));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            graph_signature(app, owner_a),
            accepted_signature,
            "stale same-revision squash evidence mutated the accepted graph/lane/edge signature"
        );
        assert!(
            ghost_rows(app, owner_a).is_empty(),
            "stale same-revision squash evidence attached a ghost edge"
        );
    });

    let fresh = queue_scan(cx);
    app.update(cx, |app, cx| app.start_squash_link_scan(cx));
    fresh.send(Ok(vec![link(
        &old_head,
        &new_head,
        "fresh-published-model",
    )]));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            ghost_rows(app, owner_a),
            vec![new_head.0.clone(), old_head.0.clone()],
            "fresh squash evidence did not decorate the accepted graph"
        );
        assert_ne!(
            graph_signature(app, owner_a),
            accepted_signature,
            "fresh squash evidence left the graph signature unchanged"
        );
    });

    drop(scan_rearm_hold);
    unmount(cx, app, window);
}

fn amend_invalidates_same_revision_scan(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    cx.run_until_parked();

    let owner = app.update(cx, |app, _| app.active_session().expect("active fixture"));
    let scan_rearm_hold = app.update(cx, |_, cx| {
        cx.observe(&app, |app, _, _| app.scans_stale = false)
    });

    let stale = queue_scan(cx);
    let (scan_revision, scan_gen, scan_publish_gen, old_head, old_root) =
        app.update(cx, |app, cx| {
            let _read_key = app.reads.begin(owner);
            let scan_revision = app.reads.revision(owner);
            let rows = &app.reads.get(Some(owner)).rows;
            let old_head = rows.first().expect("head row").id.clone();
            let old_root = rows.last().expect("root row").id.clone();
            app.start_squash_link_scan(cx);
            (
                scan_revision,
                app.squash_gen,
                app.ui[&owner].view_publish_gen,
                old_head,
                old_root,
            )
        });

    std::fs::write(repo.join("amended-model.txt"), "amended model\n").unwrap();
    git(&repo, &["add", "amended-model.txt"]);
    git(
        &repo,
        &["commit", "-q", "-m", "advance amended squash model"],
    );
    let amended_view = build_view(&repo);
    let new_head = amended_view.rows[0].id.clone();
    assert_ne!(
        new_head, old_head,
        "real commit did not rebuild amended graph"
    );

    let amended_signature = app.update(cx, |app, _| {
        app.amend_tab_view(owner, amended_view);
        assert_eq!(
            app.reads.revision(owner),
            scan_revision,
            "amend advanced the read revision"
        );
        assert_eq!(
            app.squash_gen, scan_gen,
            "amend launched a replacement scan before stale settlement"
        );
        assert!(
            app.ui[&owner].view_publish_gen > scan_publish_gen,
            "amend did not advance the published-model generation"
        );
        assert_eq!(
            app.reads.get(Some(owner)).rows[0].id,
            new_head,
            "amend did not publish the real commit"
        );
        graph_signature(app, owner)
    });

    stale.send(Ok(vec![link(&old_root, &old_head, "stale-amended-model")]));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            graph_signature(app, owner),
            amended_signature,
            "stale same-revision squash evidence mutated the amended graph/lane/edge signature"
        );
        assert!(
            ghost_rows(app, owner).is_empty(),
            "stale same-revision squash evidence attached a ghost edge after amend"
        );
    });

    let fresh = queue_scan(cx);
    app.update(cx, |app, cx| app.start_squash_link_scan(cx));
    fresh.send(Ok(vec![link(&old_head, &new_head, "fresh-amended-model")]));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            ghost_rows(app, owner),
            vec![new_head.0.clone(), old_head.0.clone()],
            "fresh squash evidence did not decorate the amended graph"
        );
    });

    drop(scan_rearm_hold);
    unmount(cx, app, window);
}

pub fn scenario_squash_evidence_publish_generation(cx: &mut VisualTestAppContext) {
    accept_while_background_invalidates_same_revision_scan(cx);
    amend_invalidates_same_revision_scan(cx);
}
