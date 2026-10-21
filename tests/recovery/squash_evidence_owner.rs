//! #717: squash evidence settles only against the read revision it scanned.
use crate::evidence_support::{ghost_rows, graph_signature, link, queue_scan};
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;

/// A completed scan is evidence for the read at launch, not merely for the tab
/// and global squash generation. Keep render-driven replacement scans held off
/// so the old completion is decided by the read-revision guard itself.
pub fn scenario_squash_evidence_read_revision(cx: &mut VisualTestAppContext) {
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

    // Hold render-driven replacement scans across read-completion notifications,
    // modeling the old callback arriving before the next scan launch. Neither
    // generation nor read revision is modified by this scheduling control.
    let scan_rearm_hold = app.update(cx, |_, cx| {
        cx.observe(&app, |app, _, _| app.scans_stale = false)
    });

    let stale = queue_scan(cx);
    let (scan_gen, scan_revision, old_head, old_root) = app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        let rows = &app.reads.get(Some(owner_a)).rows;
        let old_head = rows.first().expect("A head row").id.clone();
        let old_root = rows.last().expect("A root row").id.clone();
        app.start_squash_link_scan(cx);
        let scan_gen = app.squash_gen;
        let scan_revision = app.reads.revision(owner_a);
        app.switch_repo(1, cx);
        (scan_gen, scan_revision, old_head, old_root)
    });
    cx.run_until_parked();

    // Add a real commit, then launch A's real reload and leave it in the
    // background before the dispatcher can accept the read.
    std::fs::write(repo_a.join("revision.txt"), "new read\n").unwrap();
    git(&repo_a, &["add", "revision.txt"]);
    git(
        &repo_a,
        &["commit", "-q", "-m", "advance squash evidence read"],
    );
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        app.reload(cx);
        app.switch_repo(1, cx);
    });
    cx.run_until_parked();

    let (new_head, background_signature) = app.update(cx, |app, _| {
        assert_eq!(
            app.active_session(),
            Some(owner_b),
            "A read stayed background"
        );
        assert_eq!(
            app.squash_gen, scan_gen,
            "a replacement scan masked the read-revision guard",
        );
        assert!(
            app.reads.revision(owner_a) > scan_revision,
            "the real background reload did not advance A's read revision",
        );
        let new_head = app.reads.get(Some(owner_a)).rows[0].id.clone();
        assert_ne!(
            new_head, old_head,
            "the accepted read did not renumber the graph"
        );
        assert!(
            ghost_rows(app, owner_a).is_empty(),
            "the new graph already contained stale squash semantics",
        );
        (new_head, graph_signature(app, owner_a))
    });

    // Return through the real tab lifecycle before the old result lands. Hold
    // the render re-arm off until this completion is observed; otherwise its
    // generation bump would test the older guard instead of read freshness.
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
    });
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            app.squash_gen, scan_gen,
            "returning to A launched a replacement scan before stale settlement",
        );
        assert_eq!(
            graph_signature(app, owner_a),
            background_signature,
            "returning to A changed the accepted graph before stale settlement",
        );
    });

    // Both OIDs still exist in the live index and old-head is above old-root, so
    // without the revision guard this stale semantic link is observably drawn on
    // the new graph. The transport never carries row indices.
    stale.send(Ok(vec![link(&old_root, &old_head, "stale-semantic-link")]));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            graph_signature(app, owner_a),
            background_signature,
            "stale squash evidence mutated the accepted graph/ghost-edge signature",
        );
        assert!(
            ghost_rows(app, owner_a).is_empty(),
            "stale squash evidence attached a ghost edge to the new graph",
        );
    });

    // The same production callback still accepts evidence captured from the
    // current read. This link joins the new HEAD to its live predecessor.
    let fresh = queue_scan(cx);
    app.update(cx, |app, cx| {
        app.start_squash_link_scan(cx);
    });
    fresh.send(Ok(vec![link(&old_head, &new_head, "fresh-semantic-link")]));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            ghost_rows(app, owner_a),
            vec![new_head.0.clone(), old_head.0.clone()],
            "fresh squash evidence did not decorate the current graph",
        );
        assert_ne!(
            graph_signature(app, owner_a),
            background_signature,
            "fresh squash evidence left the graph signature unchanged",
        );
    });

    drop(scan_rearm_hold);
    unmount(cx, app, window);
}
