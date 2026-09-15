//! #643 S2b: Analyze evidence settles into its frozen session and request.
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{ClipboardItem, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_domain::hotspot::{CommitChanges, FileChange, RawEcosystem};
use std::collections::BTreeMap;

fn raw(path: &str) -> RawEcosystem {
    RawEcosystem {
        commits: vec![CommitChanges {
            time: 1,
            author: "evidence@example.test".into(),
            files: vec![FileChange {
                path: path.into(),
                insertions: 7,
                deletions: 0,
            }],
        }],
        loc: BTreeMap::from([(path.to_string(), 7)]),
    }
}

fn copy_diagnostic(cx: &mut VisualTestAppContext, app: &gpui::Entity<kagi::ui::KagiApp>) -> String {
    app.update(cx, |app, cx| {
        app.ui()
            .ecosystem
            .clone()
            .expect("Analyze pane")
            .update(cx, |pane, cx| pane.copy_diagnostic(cx));
    });
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .expect("Analyze diagnostic clipboard text")
}

pub fn scenario_ecosystem_evidence_background_owner(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo_a);
    let owner_b = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        app.active_session().expect("B owner")
    });
    cx.run_until_parked();

    let (task_a, reply_a) = deferred(cx);
    e2e::queue_ecosystem_mine(task_a);
    let owner_a = app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        let owner = app.active_session().expect("A owner");
        app.open_ecosystem_view(cx);
        owner
    });
    let (task_b, reply_b) = deferred(cx);
    e2e::queue_ecosystem_mine(task_b);
    app.update(cx, |app, cx| {
        app.switch_repo(1, cx);
        assert_eq!(app.active_session(), Some(owner_b));
        app.open_ecosystem_view(cx);
    });

    reply_b.send(Ok(raw("beta-evidence.rs")));
    cx.run_until_parked();
    let beta_before = copy_diagnostic(cx, &app);
    assert!(
        beta_before.contains("beta-evidence.rs"),
        "B result must seed B's active Analyze pane"
    );

    reply_a.send(Ok(raw("alpha-evidence.rs")));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        let a = app.ui.get(&owner_a).expect("A UI owner");
        let b = app.ui.get(&owner_b).expect("B UI owner");
        assert!(
            a.ecosystem_cache
                .as_ref()
                .is_some_and(|cache| cache.raw.loc.contains_key("alpha-evidence.rs")),
            "A background result must settle into A's cache"
        );
        assert!(
            b.ecosystem_cache
                .as_ref()
                .is_some_and(|cache| cache.raw.loc.contains_key("beta-evidence.rs")),
            "A completion must not replace B's cache"
        );
    });
    let beta_after = copy_diagnostic(cx, &app);
    assert!(
        beta_after.contains("beta-evidence.rs") && !beta_after.contains("alpha-evidence.rs"),
        "A completion must not replace B's active Analyze pane"
    );

    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        app.open_ecosystem_view(cx);
    });
    let alpha = copy_diagnostic(cx, &app);
    assert!(
        alpha.contains("alpha-evidence.rs"),
        "returning to A must restore A's accepted cache"
    );
    unmount(cx, app, window);
}

pub fn scenario_ecosystem_evidence_superseded(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (older_task, older_reply) = deferred(cx);
    e2e::queue_ecosystem_mine(older_task);
    let owner = app.update(cx, |app, cx| {
        app.open_ecosystem_view(cx);
        app.active_session().expect("Analyze owner")
    });

    std::fs::write(repo.join("new-head.txt"), "new head\n").unwrap();
    git(&repo, &["add", "new-head.txt"]);
    git(&repo, &["commit", "-q", "-m", "advance Analyze HEAD"]);
    let (newer_task, newer_reply) = deferred(cx);
    e2e::queue_ecosystem_mine(newer_task);
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();

    older_reply.send(Ok(raw("obsolete-head.rs")));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        let ui = app.ui.get(&owner).expect("Analyze owner retained");
        assert!(
            ui.ecosystem_inflight,
            "older completion must not clear the newer Analyze flight"
        );
        assert!(
            ui.ecosystem_cache.is_none(),
            "older completion must not populate the superseded cache"
        );
    });
    app.update(cx, |_app, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("pane-still-loading".into()));
    });
    app.update(cx, |app, cx| {
        app.ui()
            .ecosystem
            .clone()
            .expect("Analyze pane")
            .update(cx, |pane, cx| pane.copy_diagnostic(cx));
    });
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("pane-still-loading"),
        "older completion must not seed the superseded Analyze pane"
    );

    newer_reply.send(Ok(raw("current-head.rs")));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        let ui = app.ui.get(&owner).expect("Analyze owner retained");
        assert!(
            !ui.ecosystem_inflight,
            "newest completion settles its flight"
        );
        assert!(
            ui.ecosystem_cache
                .as_ref()
                .is_some_and(|cache| cache.raw.loc.contains_key("current-head.rs")),
            "newest HEAD result must own the Analyze cache"
        );
    });
    let current = copy_diagnostic(cx, &app);
    assert!(
        current.contains("current-head.rs") && !current.contains("obsolete-head.rs"),
        "newest HEAD result must seed the Analyze pane"
    );
    unmount(cx, app, window);
}

pub fn scenario_ecosystem_evidence_detached_samepath(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (old_task, old_reply) = deferred(cx);
    e2e::queue_ecosystem_mine(old_task);
    let old_owner = app.update(cx, |app, cx| {
        app.open_ecosystem_view(cx);
        let owner = app.active_session().expect("old owner");
        app.close_tab(0, cx);
        owner
    });
    assert!(
        cx.read(|cx| !app.read(cx).ui.contains_key(&old_owner)),
        "old owner detached"
    );

    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo.clone(), cx), "reopen same path");
    });
    cx.run_until_parked();
    let (new_task, new_reply) = deferred(cx);
    e2e::queue_ecosystem_mine(new_task);
    let new_owner = app.update(cx, |app, cx| {
        let owner = app.active_session().expect("new owner");
        assert_ne!(owner, old_owner, "same path must get a new incarnation");
        app.open_ecosystem_view(cx);
        owner
    });

    old_reply.send(Ok(raw("detached-old.rs")));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert!(
            !app.ui.contains_key(&old_owner),
            "detached completion must not recreate its released owner"
        );
        let ui = app.ui.get(&new_owner).expect("new owner retained");
        assert!(
            ui.ecosystem_inflight,
            "detached completion must not clear the new incarnation's flight"
        );
        assert!(
            ui.ecosystem_cache.is_none(),
            "detached completion must not enter the same-path incarnation"
        );
    });

    new_reply.send(Ok(raw("reopened-current.rs")));
    cx.run_until_parked();
    let current = copy_diagnostic(cx, &app);
    assert!(
        current.contains("reopened-current.rs") && !current.contains("detached-old.rs"),
        "the reopened pane must show only its incarnation's result"
    );
    unmount(cx, app, window);
}
