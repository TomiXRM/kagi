//! #482 stage 2 (ADR-0183): the read model has one owner, and switching to it
//! costs a reference — not a copy.
//!
//! Everything here is observed on the real root against real repositories:
//! two fixtures opened as two tabs, an A→B→A round trip, then a reload, a
//! load-more and a solo toggle on the tab that came back. What it records is
//! ownership, not pixels — the allocation identity of the owner's rows, the
//! process-wide clone/build/drop counters, and the row/detail/index agreement
//! after each renumber. No RSS number is claimed: the counters say what was
//! copied and what was freed, which is the property the design promises.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::KagiApp;
use kagi_git::CommitId;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `main` with three commits plus a `side` branch two commits ahead, so soloing
/// `main` really hides rows instead of being a no-op.
fn build_branching_fixture(root: &Path, name: &str) -> PathBuf {
    let repo = root.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for i in 0..3 {
        std::fs::write(repo.join("f.txt"), format!("{name} main {i}\n")).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", &format!("{name} main {i}")]);
    }
    git(&repo, &["checkout", "-q", "-b", "side"]);
    for i in 0..2 {
        std::fs::write(repo.join("f.txt"), format!("{name} side {i}\n")).unwrap();
        git(&repo, &["commit", "-q", "-am", &format!("{name} side {i}")]);
    }
    git(&repo, &["checkout", "-q", "main"]);
    repo.canonicalize().unwrap()
}

/// What the app's read store published, deep-copied and freed so far.
fn counters(
    cx: &mut VisualTestAppContext,
    kagi: &gpui::Entity<KagiApp>,
) -> kagi::app::ReadCounters {
    kagi.update(cx, |app, _| app.reads.counters())
}

/// Address of the owner's row vector. Taken through a borrow and never held: a
/// retained `Arc` clone is precisely what would force the copy-on-write this
/// scenario asserts does not happen.
fn rows_ptr(app: &KagiApp, session: kagi::app::SessionId) -> usize {
    app.reads
        .get(Some(session))
        .rows
        .as_ptr()
        .cast::<u8>()
        .addr()
}

/// Every row's detail and index entry names that row's own commit. The single
/// invariant every renumber (#286) has to preserve.
fn assert_rows_consistent(app: &KagiApp, where_: &str) {
    let view = app.view();
    for (index, row) in view.rows.iter().enumerate() {
        assert_eq!(
            view.commit_row_index.get(&row.id),
            Some(&index),
            "{where_}: row {index} is not indexed at its own position",
        );
        assert_eq!(
            view.details[index].full_sha.as_ref(),
            row.id.0.as_str(),
            "{where_}: the detail at row {index} belongs to another commit",
        );
    }
}

pub fn scenario_read_owner_switch(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_branching_fixture(&root, "alpha");
    let repo_b = build_branching_fixture(&root, "beta");
    // `build_fixture` is what every other scenario mounts against; keeping the
    // helper referenced here documents that this one deliberately does not.
    let _ = build_fixture;

    let (kagi, window) = mount(cx, &repo_a);

    let (session_a, session_b) = kagi.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        (app.tabs[0].session, app.tabs[1].session)
    });
    assert_ne!(session_a, session_b, "each tab is its own owner");
    cx.run_until_parked();

    // Both tabs' first reads have landed, on their own owners.
    let (a_rows, b_rows) = kagi.update(cx, |app, _| {
        assert_eq!(app.active_session(), Some(session_b));
        assert_eq!(app.reads.get(Some(session_a)).rows.len(), 5, "A loaded");
        assert_eq!(app.reads.get(Some(session_b)).rows.len(), 5, "B loaded");
        (rows_ptr(app, session_a), rows_ptr(app, session_b))
    });

    // ── A→B→A: the switch is a change of key, nothing else ────────────────
    //
    // Measured *before* parking: `switch_repo` always kicks off a background
    // revalidate, and that revalidate is a genuinely new read. What must cost
    // nothing is the swap itself.
    let before_switch = counters(cx, &kagi);
    kagi.update(cx, |app, cx| {
        app.switch_repo(0, cx); // → A
        assert_eq!(app.active_session(), Some(session_a));
        assert_eq!(
            rows_ptr(app, session_a),
            a_rows,
            "A's rows were reallocated"
        );
        assert_eq!(app.view().rows.len(), 5);
        assert!(app.view().rows[0].summary.contains("alpha"));
        app.switch_repo(1, cx); // → B
        assert_eq!(
            rows_ptr(app, session_b),
            b_rows,
            "B's rows were reallocated"
        );
        assert!(app.view().rows[0].summary.contains("beta"));
        app.switch_repo(0, cx); // → A again
        assert_eq!(rows_ptr(app, session_a), a_rows, "A→B→A rebuilt A's rows");
        assert!(app.view().rows[0].summary.contains("alpha"));
    });
    let after_switch = counters(cx, &kagi);
    assert_eq!(
        after_switch, before_switch,
        "a tab switch published, copied or freed a read model",
    );
    cx.run_until_parked();

    // The revalidate that the round trip armed is a new read for A, and it
    // replaced (and freed) exactly one older one.
    let after_revalidate = counters(cx, &kagi);
    assert!(
        after_revalidate.builds > after_switch.builds,
        "the revalidate published nothing",
    );
    assert_eq!(
        after_revalidate.copies, after_switch.copies,
        "the revalidate deep-copied a read model",
    );
    assert!(
        after_revalidate.drops > after_switch.drops,
        "the superseded read was not freed",
    );
    kagi.update(cx, |app, _| {
        assert_eq!(app.active_session(), Some(session_a));
        assert_eq!(app.view().rows.len(), 5);
        assert_rows_consistent(app, "after A→B→A");
    });

    // ── reload: a new read for the same owner ─────────────────────────────
    let selected = kagi.update(cx, |app, cx| {
        app.select(1);
        let id = app.view().rows[1].id.clone();
        app.reload_checked(cx).expect("reload");
        id
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_eq!(app.view().rows.len(), 5, "reload kept the graph");
        assert_rows_consistent(app, "after reload");
        // The selection was re-resolved by OID, and the detail under it is that
        // commit's — the "selected OID and diff content agree" acceptance.
        let row = app.selected.expect("selection survived the reload");
        assert_eq!(app.view().rows[row].id, selected);
        assert_eq!(
            app.view().details[row].full_sha.as_ref(),
            selected.0.as_str(),
        );
    });

    // ── load-more: the page grows, existing rows keep their indices ───────
    let before: Vec<CommitId> = kagi.update(cx, |app, cx| {
        app.commit_limit = 2;
        app.reload_checked(cx).expect("reload at the smaller limit");
        app.view().rows.iter().map(|r| r.id.clone()).collect()
    });
    cx.run_until_parked();
    kagi.update(cx, |app, cx| {
        assert_eq!(app.view().rows.len(), 2, "the page was truncated");
        app.load_more_commits(cx);
        assert_eq!(app.view().rows.len(), 5, "load-more did not grow the page");
        for (index, id) in before.iter().enumerate() {
            assert_eq!(&app.view().rows[index].id, id, "row {index} was renumbered");
        }
        assert_rows_consistent(app, "after load-more");
    });
    cx.run_until_parked();

    // ── solo: rows are filtered in place and restored from the saved set ───
    kagi.update(cx, |app, cx| {
        app.commit_limit = 10_000;
        app.reload_checked(cx).expect("reload at the full limit");
    });
    cx.run_until_parked();
    let (copies_3, _) = kagi.update(cx, |app, cx| {
        let target = app.view().branch_targets["main"].clone();
        let full = app.view().rows.len();
        let copies = app.reads.counters().copies;
        app.toggle_branch_solo("main".into(), target.clone(), cx);
        assert!(
            app.view().rows.len() < full,
            "solo did not hide the side branch"
        );
        assert!(app.view().branch_solo.is_some());
        assert_rows_consistent(app, "with solo on");
        app.toggle_branch_solo("main".into(), target, cx);
        assert_eq!(app.view().rows.len(), full, "solo off did not restore");
        assert!(app.view().branch_solo.is_none());
        assert_rows_consistent(app, "with solo off");
        (copies, full)
    });
    assert_eq!(
        counters(cx, &kagi).copies,
        copies_3,
        "an in-place solo toggle deep-copied the read model",
    );

    // ── close: the last reference to each owner's read is released ────────
    let drops_before_close = counters(cx, &kagi).drops;
    kagi.update(cx, |app, cx| {
        app.close_tab(1, cx); // background B
        assert!(app.reads.share(session_b).is_none(), "B's read outlived B");
        app.close_tab(0, cx); // the last tab → Welcome
        assert!(app.reads.share(session_a).is_none(), "A's read outlived A");
        assert!(app.tabs.is_empty());
        assert!(app.view().rows.is_empty(), "Welcome shows the empty read");
    });
    assert_eq!(
        counters(cx, &kagi).drops,
        drops_before_close + 2,
        "closing two tabs did not free both read models",
    );

    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] read_owner_switch: OK");
}

/// `main` and `side` that both changed the same line, so merging `side` leaves a
/// real index conflict for the watcher path to re-detect.
fn build_conflict_fixture(root: &Path, name: &str) -> PathBuf {
    let repo = root.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("f.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("f.txt"), "side\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "side edit"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("f.txt"), "main\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "main edit"]);
    repo.canonicalize().unwrap()
}

/// `git` that is allowed to fail — a conflicting merge exits non-zero *because*
/// it worked.
fn git_expect_conflict(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "poc")
        .env("GIT_AUTHOR_EMAIL", "poc@example.com")
        .env("GIT_COMMITTER_NAME", "poc")
        .env("GIT_COMMITTER_EMAIL", "poc@example.com")
        .status()
        .expect("spawn git");
    assert!(!status.success(), "{args:?} was supposed to conflict");
}

/// The three orderings the stage-2 review found unguarded: a reload arriving
/// during the very first load, paging landing on top of a pending full reload,
/// and a remote re-snapshot replacing an incarnation. Each is a *sequence*, not
/// a state, so each is driven on the real root rather than asserted at the store.
pub fn scenario_read_owner_ordering(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_conflict_fixture(&root, "alpha");
    let repo_b = build_branching_fixture(&root, "beta");

    let (kagi, window) = mount(cx, &repo_a);
    cx.run_until_parked();

    // ── item 2: Cmd+R during the first Loading of a not-yet-arrived tab ────
    //
    // As a stored field the placeholder was set by the switch and cleared by the
    // load that switch started; the reload refused that load and cleared
    // nothing, so `Loading …` stayed on screen forever. (The failing direction
    // — the replacing reload itself errors — is covered in `src/app/read.rs`,
    // where a broken repository can be simulated without a window.)
    let session_b = kagi.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        let session = app.active_session().expect("B is on screen");
        assert!(
            app.loading_tab().is_some(),
            "B's first read is in flight — the placeholder must be up",
        );
        // A perfectly legal manual refresh, before the first read lands.
        app.reload_checked(cx).expect("Cmd+R during the first load");
        assert!(
            app.loading_tab().is_none(),
            "the replacing reload must settle the placeholder",
        );
        assert!(!app.view().rows.is_empty(), "the reload filled the tab");
        assert!(
            !matches!(app.status_footer, kagi::ui::FooterStatus::Busy(_)),
            "the Loading footer outlived the read that set it",
        );
        session
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        // The superseded first load has now landed and was refused; it must not
        // have put the placeholder back.
        assert!(app.loading_tab().is_none(), "a stale read revived Loading");
        assert!(!app.view().rows.is_empty());
        assert_eq!(app.active_session(), Some(session_b));
    });

    // ── item 3: Load more must not supersede a pending full reload ─────────
    //
    // The reload is the read that carries Conflict Mode re-detection and the
    // working-tree baseline; paging only has rows. If paging bumped the
    // revision, the conflict would go unnoticed until something else refreshed.
    kagi.update(cx, |app, cx| {
        app.switch_repo(0, cx); // → A
    });
    cx.run_until_parked();
    git_expect_conflict(&repo_a, &["merge", "side"]);
    kagi.update(cx, |app, cx| {
        assert!(
            app.last_working_status
                .as_ref()
                .is_some_and(|s| s.conflicted.is_empty()),
            "the baseline should predate the conflict",
        );
        app.reload_external(cx); // the watcher's full reload starts
        app.commit_limit = 1;
        app.load_more_commits(cx); // and paging lands first
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert!(
            app.view().status_summary.conflict_count > 0,
            "the full reload was refused by paging — the conflict went unseen",
        );
        assert!(
            app.last_working_status
                .as_ref()
                .is_some_and(|s| !s.conflicted.is_empty()),
            "the reload's working-tree baseline never landed",
        );
        assert_rows_consistent(app, "after conflict reload + paging");
    });
    git(&repo_a, &["merge", "--abort"]);

    // ── item 4: every remote re-snapshot releases the incarnation it replaces ─
    //
    // `reattach` issues a new `SessionId`; without releasing the old one, each
    // refresh left a full rows/details set keyed under an id no tab names, and
    // `close_tab` only ever reached the current one.
    let host = kagi_domain::remote::RemoteHost::parse("example.test").expect("host");
    let remote_root = "/srv/beta".to_string();
    let snapshot = || {
        let mut backend = kagi_git::Backend::open(&repo_b).expect("open");
        backend.snapshot(10_000).expect("snapshot")
    };
    kagi.update(cx, |app, cx| {
        app.enter_remote_view(host.clone(), remote_root.clone(), snapshot(), cx);
    });
    cx.run_until_parked();
    for round in 1..=2 {
        let drops_before = counters(cx, &kagi).drops;
        kagi.update(cx, |app, cx| {
            app.enter_remote_view(host.clone(), remote_root.clone(), snapshot(), cx);
        });
        assert_eq!(
            counters(cx, &kagi).drops,
            drops_before + 1,
            "remote refresh {round} leaked the previous incarnation's read",
        );
    }
    cx.run_until_parked();

    let drops_before_close = counters(cx, &kagi).drops;
    let remote_tab = kagi.update(cx, |app, _| {
        app.tabs
            .iter()
            .position(|t| t.remote.is_some())
            .expect("the remote tab is open")
    });
    kagi.update(cx, |app, cx| app.close_tab(remote_tab, cx));
    assert_eq!(
        counters(cx, &kagi).drops,
        drops_before_close + 1,
        "closing the remote tab did not release its read",
    );

    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] read_owner_ordering: OK");
}
