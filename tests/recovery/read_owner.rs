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
use kagi::app::read_counters;
use kagi::ui::KagiApp;
use kagi_git::CommitId;
use std::path::{Path, PathBuf};

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
    let (builds_0, copies_0, drops_0) = read_counters();
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
    let (builds_1, copies_1, drops_1) = read_counters();
    assert_eq!(builds_1, builds_0, "a tab switch published a read");
    assert_eq!(copies_1, copies_0, "a tab switch deep-copied a read model");
    assert_eq!(drops_1, drops_0, "a tab switch freed a read model");
    cx.run_until_parked();

    // The revalidate that the round trip armed is a new read for A, and it
    // replaced (and freed) exactly one older one.
    let (builds_2, copies_2, drops_2) = read_counters();
    assert!(builds_2 > builds_1, "the revalidate published nothing");
    assert_eq!(
        copies_2, copies_1,
        "the revalidate deep-copied a read model"
    );
    assert!(drops_2 > drops_1, "the superseded read was not freed");
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
        let copies = read_counters().1;
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
        read_counters().1,
        copies_3,
        "an in-place solo toggle deep-copied the read model",
    );

    // ── close: the last reference to each owner's read is released ────────
    let drops_before_close = read_counters().2;
    kagi.update(cx, |app, cx| {
        app.close_tab(1, cx); // background B
        assert!(app.reads.share(session_b).is_none(), "B's read outlived B");
        app.close_tab(0, cx); // the last tab → Welcome
        assert!(app.reads.share(session_a).is_none(), "A's read outlived A");
        assert!(app.tabs.is_empty());
        assert!(app.view().rows.is_empty(), "Welcome shows the empty read");
    });
    assert_eq!(
        read_counters().2,
        drops_before_close + 2,
        "closing two tabs did not free both read models",
    );

    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] read_owner_switch: OK");
}
