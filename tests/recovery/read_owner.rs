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

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_succeeds;

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
        app.reload_manual(cx);
        id
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_eq!(app.view().rows.len(), 5, "reload kept the graph");
        assert_rows_consistent(app, "after reload");
        // The selection was re-resolved by OID, and the detail under it is that
        // commit's — the "selected OID and diff content agree" acceptance.
        let row = app.ui().selected.expect("selection survived the reload");
        assert_eq!(app.view().rows[row].id, selected);
        assert_eq!(
            app.view().details[row].full_sha.as_ref(),
            selected.0.as_str(),
        );
    });

    // ── load-more: the page grows, existing rows keep their indices ───────
    kagi.update(cx, |app, cx| {
        app.ui_mut().expect("active session").commit_limit = 2;
        app.reload_manual(cx);
    });
    cx.run_until_parked();
    let before: Vec<CommitId> = kagi.update(cx, |app, cx| {
        assert_eq!(app.view().rows.len(), 2, "the page was truncated");
        app.load_more_commits(cx); // #487: background read
        app.view().rows.iter().map(|r| r.id.clone()).collect()
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_eq!(app.view().rows.len(), 5, "load-more did not grow the page");
        for (index, id) in before.iter().enumerate() {
            assert_eq!(&app.view().rows[index].id, id, "row {index} was renumbered");
        }
        assert_rows_consistent(app, "after load-more");
    });

    // ── solo: rows are filtered in place and restored from the saved set ───
    kagi.update(cx, |app, cx| {
        app.ui_mut().expect("active session").commit_limit = 10_000;
        app.reload_manual(cx);
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
    assert!(
        !git_succeeds(repo, args),
        "{args:?} was supposed to conflict"
    );
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
        // #487: it is a background read too now, so its own landing is what
        // settles the placeholder — asserted once everything has parked.
        app.reload_manual(cx);
        session
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        // The manual reload landed and the superseded first load was refused;
        // the refusal must not have put the placeholder back.
        assert!(
            app.loading_tab().is_none(),
            "the replacing reload must settle the placeholder",
        );
        assert!(!app.view().rows.is_empty(), "the reload filled the tab");
        assert!(
            !matches!(app.status_footer, kagi::ui::FooterStatus::Busy(_)),
            "the Loading footer outlived the read that set it",
        );
        assert_eq!(app.active_session(), Some(session_b));
    });

    // ── item 3: Load more must not supersede a pending full reload ─────────
    //
    // The reload is the read that carries Conflict Mode re-detection and the
    // working-tree baseline; paging only has rows. If paging bumped the
    // revision, the conflict would go unnoticed until something else refreshed.
    //
    // Every step here is driven explicitly. The watcher is armed on A, but its
    // loop sleeps on `background_executor().timer` and `run_until_parked` does
    // not advance the test clock, so it never ticks: the only reloads in this
    // block are the `reload_external` below and the paging read (#487: also a
    // background read now; which of the two lands first is the dispatcher's
    // choice, and neither order may refuse the reload).
    kagi.update(cx, |app, cx| {
        app.switch_repo(0, cx); // → A
    });
    cx.run_until_parked();

    // Freeze the baseline from the *settled* read, before the repository
    // changes. (`switch_repo` clears `last_working_status` and the tab load does
    // not refill it — only a reload does — so the read model is what says what
    // the app has actually seen.)
    let session_a = kagi.update(cx, |app, cx| {
        assert_eq!(
            app.view().status_summary.conflict_count,
            0,
            "the fixture must start un-conflicted",
        );
        // One row on screen, so paging below has something to grow.
        app.ui_mut().expect("active session").commit_limit = 1;
        app.reload_manual(cx);
        app.active_session().expect("A is on screen")
    });
    cx.run_until_parked();
    let baseline_rows = kagi.update(cx, |app, _| app.view().rows.len());
    assert_eq!(baseline_rows, 1, "the page was not truncated");
    kagi.update(cx, |app, _| {
        assert_eq!(
            app.view().status_summary.conflict_count,
            0,
            "the baseline read predates the conflict",
        );
        assert!(
            app.ui()
                .last_working_status
                .as_ref()
                .is_some_and(|s| s.conflicted.is_empty()),
            "the reload's baseline predates the conflict",
        );
    });

    // Now the external change, and the two reads racing over it.
    git_expect_conflict(&repo_a, &["merge", "side"]);
    let revision_before = kagi.update(cx, |app, cx| {
        app.reload_external(cx); // the full reload starts (async)
        let revision = app.reads.revision(session_a);
        app.load_more_commits(cx); // paging starts (async, #487)
                                   // Paging must not bump the owner's revision — that is the whole
                                   // "amend, never supersede" rule. The rest is asserted once both land.
        assert_eq!(
            app.reads.revision(session_a),
            revision,
            "paging superseded the pending full reload",
        );
        assert!(
            app.ui()
                .last_working_status
                .as_ref()
                .is_some_and(|s| s.conflicted.is_empty()),
            "the reload applied before anything could race it",
        );
        revision
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_eq!(app.reads.revision(session_a), revision_before);
        assert!(
            app.view().status_summary.conflict_count > 0,
            "the full reload was refused by paging — the conflict went unseen",
        );
        assert!(
            app.ui()
                .last_working_status
                .as_ref()
                .is_some_and(|s| !s.conflicted.is_empty()),
            "the reload's working-tree baseline never landed",
        );
        // Whichever read landed last owns the row count (the reload captured
        // the small limit; paging the raised one), but neither may leave fewer
        // rows than the settled baseline.
        assert!(
            app.view().rows.len() >= baseline_rows,
            "a stale page shrank the graph below its settled baseline",
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

/// #487: commit-graph paging and the manual refresh are background reads. A
/// result is applied only for the owner that asked and only while its request
/// is the owner's latest — so a spammed click, a refresh racing a page, and a
/// tab switch mid-page can never land a stale page, and the active tab never
/// receives a background owner's rows. A failed page reaches the user through
/// the existing footer + toast contract, not only the log.
///
/// The witness for "not applied" is `ReadCounters::builds`, which every store
/// (accept *or* amend) increments exactly once: the number of reads that
/// landed is observed directly rather than inferred from row counts, which
/// two pages of one 5-commit fixture could not tell apart.
pub fn scenario_load_more_stale_reads(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_branching_fixture(&root, "alpha");
    let repo_b = build_branching_fixture(&root, "beta");
    let (kagi, window) = mount(cx, &repo_a);
    cx.run_until_parked();

    // A truncated page, settled: one row on screen out of five.
    let truncate = |cx: &mut VisualTestAppContext, kagi: &gpui::Entity<KagiApp>| {
        kagi.update(cx, |app, cx| {
            app.ui_mut().expect("active session").commit_limit = 1;
            app.reload_manual(cx);
        });
        cx.run_until_parked();
        kagi.update(cx, |app, _| {
            assert_eq!(app.view().rows.len(), 1, "the page was not truncated");
        });
    };

    // ── spam: two clicks in flight, only the latest page lands ──────────────
    truncate(cx, &kagi);
    kagi::ui::list_a11y::clear_recorded_lists();
    kagi.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let load_more = kagi::ui::list_a11y::recorded_list("commit-load-more")
        .expect("truncated graph renders its load-more control");
    assert_eq!(load_more.role, Some(gpui::Role::Button));
    assert_eq!(
        load_more.label,
        kagi_ui_core::i18n::Msg::LoadMoreCommits.t()
    );
    let list = kagi::ui::list_a11y::recorded_list("commit-list").expect("commit ListBox");
    assert_eq!(list.size, 1, "pagination is not a selectable commit");
    // Exercise the actual Button, rather than only calling the page method:
    // a truncated list must keep its action reachable outside the options.
    let bounds = kagi::ui::e2e::control_bounds(window.window_id(), "commit-load-more")
        .expect("Load More button was not laid out");
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| kagi.read(cx).view().rows.len()), 5);
    kagi::ui::list_a11y::clear_recorded_lists();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    assert!(
        kagi::ui::list_a11y::recorded_list("commit-load-more").is_none(),
        "the button disappears when the page reaches the end"
    );
    truncate(cx, &kagi);
    let (session_a, selected, builds_before, gen_before) = kagi.update(cx, |app, cx| {
        app.select(0);
        let selected = app.view().rows[0].id.clone();
        let builds = app.reads.counters().builds;
        let gen_before = app.ui().load_more_gen;
        app.load_more_commits(cx);
        app.load_more_commits(cx);
        assert_eq!(
            app.view().rows.len(),
            1,
            "paging blocked the UI thread: the page grew before the call returned",
        );
        (
            app.active_session().expect("A"),
            selected,
            builds,
            gen_before,
        )
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_eq!(app.view().rows.len(), 5, "the latest page did not land");
        assert_eq!(
            app.reads.counters().builds,
            builds_before + 1,
            "load-more-spam: the superseded first page was applied too",
        );
        assert_eq!(app.ui().load_more_gen, gen_before + 2);
        let row = app.ui().selected.expect("selection survived paging");
        assert_eq!(
            app.view().rows[row].id,
            selected,
            "paging moved the selection"
        );
        assert_rows_consistent(app, "after spammed load-more");
    });

    // ── refresh race: a manual refresh started after a page supersedes it ──
    truncate(cx, &kagi);
    let builds_before = kagi.update(cx, |app, cx| {
        let builds = app.reads.counters().builds;
        app.load_more_commits(cx);
        app.reload_manual(cx); // bumps the owner's revision → the page is stale
        builds
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_eq!(
            app.reads.counters().builds,
            builds_before + 1,
            "load-more-vs-refresh: the stale page landed beside the refresh",
        );
        // The refresh captured the raised limit, so it is the read that grew
        // the graph; the page had nothing left to add.
        assert_eq!(app.view().rows.len(), 5, "the refresh did not land");
        assert_rows_consistent(app, "after load-more vs refresh");
    });

    // ── tab switch: the page lands on its owner, never on the tab on screen ─
    truncate(cx, &kagi);
    let session_b = kagi.update(cx, |app, cx| {
        app.load_more_commits(cx);
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        app.active_session().expect("B is on screen")
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_eq!(app.active_session(), Some(session_b));
        assert_eq!(
            app.ui().commit_limit,
            kagi::ui::DEFAULT_COMMIT_LIMIT,
            "A's paging limit leaked into B",
        );
        assert_eq!(
            app.ui().load_more_gen,
            0,
            "A's paging request leaked into B"
        );
        assert_eq!(
            app.reads.get(Some(session_a)).rows.len(),
            5,
            "A's page did not reach its owner while A was in the background",
        );
        assert_eq!(app.view().rows.len(), 5, "B's own first read did not land");
        assert_rows_consistent(app, "B after A's background page");
    });
    kagi.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_eq!(app.active_session(), Some(session_a));
        assert_eq!(app.view().rows.len(), 5, "A's page was lost on return");
        assert_rows_consistent(app, "A after return");
    });

    // ── failure: a page that cannot open the repository says so on screen ──
    truncate(cx, &kagi);
    let git_dir = repo_a.join(".git");
    let parked = repo_a.join(".git.parked");
    std::fs::rename(&git_dir, &parked).expect("park .git");
    kagi.update(cx, |app, cx| app.load_more_commits(cx));
    cx.run_until_parked();
    std::fs::rename(&parked, &git_dir).expect("restore .git");
    kagi.update(cx, |app, cx| {
        assert_eq!(app.view().rows.len(), 1, "a failed page changed the graph");
        assert_eq!(
            app.ui().commit_limit,
            1,
            "a failed page kept the raised limit (the load-more row would vanish)",
        );
        assert!(
            matches!(&app.status_footer, kagi::ui::FooterStatus::Failed(msg) if msg.starts_with("Load more failed: repo open error")),
            "load-more-failure footer: {:?}",
            app.status_footer
        );
        // The stack is bounded (ADR-0192), so its length says nothing; the
        // newest toast must be this failure.
        let toasts = app
            .toast_stack
            .as_ref()
            .expect("mounted app has a toast stack")
            .read(cx)
            .toasts();
        assert!(
            toasts.last().is_some_and(|t| {
                t.kind == kagi::ui::ToastKind::Error
                    && t.message.starts_with("Load more failed: repo open error")
            }),
            "load-more-failure toast: {:?}",
            toasts.iter().map(|t| t.message.to_string()).collect::<Vec<_>>()
        );
    });

    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] PASS load_more_stale_reads");
}
