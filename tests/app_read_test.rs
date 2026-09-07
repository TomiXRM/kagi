//! Window-free read-model ownership contract (#482 stage 2 / ADR-0183).
//!
//! `src/app/read.rs` unit-tests the transitions on a stand-in value. This drives
//! the same store with the **real** `TabViewState` built from **real**
//! repositories, so the properties that only mean something with actual commit
//! rows — load-more renumbering, the selected OID agreeing with the detail at
//! that row, and A→B→A costing no copy of those vectors — are checked against
//! what the app really publishes.
use kagi::app::{admit, LegacyBusy, Reads, SessionId, Sessions};
use kagi::ui::{build_tab_view, TabViewState};
use kagi_git::{Backend, CommitId};
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// A repository with `commits` linear commits, named so the two fixtures in one
/// test are told apart by content rather than by tab index.
fn fixture(dir: &Path, name: &str, commits: usize) -> PathBuf {
    let repo = dir.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for i in 0..commits {
        std::fs::write(repo.join("f.txt"), format!("{name} {i}\n")).unwrap();
        git(&repo, &["add", "."]);
        git(
            &repo,
            &["commit", "-q", "-m", &format!("{name} commit {i}")],
        );
    }
    repo
}

fn view_of(repo: &Path, name: &str, limit: usize) -> TabViewState {
    let mut backend = Backend::open(repo).expect("open");
    let snap = backend.snapshot(limit).expect("snapshot");
    build_tab_view(&snap, name)
}

fn attach(sessions: &mut Sessions, repo: &Path) -> SessionId {
    sessions.attach(repo.to_path_buf())
}

/// Two tabs load at once and complete in the wrong order. Each read lands on the
/// owner that asked for it: neither is discarded for "not being the active tab",
/// and neither can overwrite the other's rows.
#[test]
fn out_of_order_completion_keeps_each_repository_s_own_rows() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo_a = fixture(&root, "alpha", 3);
    let repo_b = fixture(&root, "beta", 5);

    let mut sessions = Sessions::new();
    let a = attach(&mut sessions, &repo_a);
    let b = attach(&mut sessions, &repo_b);
    assert_ne!(a, b);

    let mut reads: Reads<TabViewState> = Reads::new();
    let key_a = reads.begin(a);
    let key_b = reads.begin(b);

    // B finishes first, A second.
    assert!(reads.accept(key_b, view_of(&repo_b, "beta", 100)));
    assert!(reads.accept(key_a, view_of(&repo_a, "alpha", 100)));

    assert_eq!(reads.get(Some(a)).rows.len(), 3);
    assert_eq!(reads.get(Some(b)).rows.len(), 5);
    assert!(reads.get(Some(a)).rows[0].summary.contains("alpha"));
    assert!(reads.get(Some(b)).rows[0].summary.contains("beta"));
}

/// The #489 shape with real data: a reload supersedes the read in flight, the
/// older one lands late, and it must neither write its rows nor clear the newer
/// request's loading state.
#[test]
fn a_stale_read_writes_nothing_and_leaves_the_newer_one_loading() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = fixture(&root, "alpha", 2);
    let mut sessions = Sessions::new();
    let owner = attach(&mut sessions, &repo);
    let mut reads: Reads<TabViewState> = Reads::new();

    let first = reads.begin(owner);
    // A commit lands, then a fresh reload starts before the first one returns.
    std::fs::write(repo.join("f.txt"), "more\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "alpha commit 2"]);
    let second = reads.begin(owner);

    let stale = view_of(&repo, "alpha", 2); // what the first read would return
    assert!(!reads.accept(first, stale));
    assert!(
        reads.get(Some(owner)).rows.is_empty(),
        "nothing was written"
    );
    assert!(reads.is_loading(owner), "the newer read is still loading");

    assert!(reads.accept(second, view_of(&repo, "alpha", 100)));
    assert!(!reads.is_loading(owner));
    assert_eq!(reads.get(Some(owner)).rows.len(), 3);
}

/// Load-more republishes the owner's read model at a larger limit. Every row
/// that was already visible keeps its index (the older commits append below), so
/// the row-index-keyed caches #286 invalidates stay consistent, and
/// `commit_row_index` agrees with `rows` and `details` at every index.
#[test]
fn load_more_renumbering_keeps_rows_details_and_index_in_agreement() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = fixture(&root, "alpha", 6);
    let mut sessions = Sessions::new();
    let owner = attach(&mut sessions, &repo);
    let mut reads: Reads<TabViewState> = Reads::new();

    reads.publish(owner, view_of(&repo, "alpha", 3));
    let before: Vec<CommitId> = reads
        .get(Some(owner))
        .rows
        .iter()
        .map(|r| r.id.clone())
        .collect();
    assert_eq!(before.len(), 3);

    reads.publish(owner, view_of(&repo, "alpha", 6));
    let view = reads.get(Some(owner));
    assert_eq!(view.rows.len(), 6, "the page grew");
    for (index, id) in before.iter().enumerate() {
        assert_eq!(&view.rows[index].id, id, "row {index} was renumbered");
    }
    for (index, row) in view.rows.iter().enumerate() {
        assert_eq!(view.commit_row_index.get(&row.id), Some(&index));
        assert_eq!(
            view.details[index].full_sha.as_ref(),
            row.id.0.as_str(),
            "the detail at row {index} belongs to another commit",
        );
    }
}

/// What the selection means: the diff panes resolve a row index against
/// `details`, so an index carried across a republish must still name the commit
/// the user picked — or be dropped. Both cases, on real history.
#[test]
fn the_selected_oid_and_the_detail_at_its_row_agree_across_a_republish() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = fixture(&root, "alpha", 4);
    let mut sessions = Sessions::new();
    let owner = attach(&mut sessions, &repo);
    let mut reads: Reads<TabViewState> = Reads::new();
    reads.publish(owner, view_of(&repo, "alpha", 100));

    // The user selects the second row and we remember its OID, exactly as
    // `apply_reload_data` does before rebuilding.
    let selected = reads.get(Some(owner)).rows[1].id.clone();

    // A new commit shifts every existing row down by one.
    std::fs::write(repo.join("f.txt"), "tip\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "alpha tip"]);
    reads.publish(owner, view_of(&repo, "alpha", 100));

    let view = reads.get(Some(owner));
    let row = *view
        .commit_row_index
        .get(&selected)
        .expect("the commit still exists");
    assert_eq!(row, 2, "the selection moved down by the new tip");
    assert_eq!(view.rows[row].id, selected);
    assert_eq!(view.details[row].full_sha.as_ref(), selected.0.as_str());

    // A commit that is gone from the graph resolves to no row at all, which is
    // how the reload path clears the selection rather than pointing it at a
    // neighbour.
    let vanished = CommitId("0".repeat(40));
    assert!(view.commit_row_index.get(&vanished).is_none());
}

/// A→B→A on real repositories changes which key is read and nothing else: same
/// allocation, no copy of the row/detail vectors. This is the property the
/// `active_view` + `tab_cache` pair could not have — it cloned the whole
/// `TabViewState` in each direction.
#[test]
fn switching_between_two_repositories_and_back_copies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo_a = fixture(&root, "alpha", 3);
    let repo_b = fixture(&root, "beta", 4);
    let mut sessions = Sessions::new();
    let a = attach(&mut sessions, &repo_a);
    let b = attach(&mut sessions, &repo_b);

    let mut reads: Reads<TabViewState> = Reads::new();
    reads.publish(a, view_of(&repo_a, "alpha", 100));
    reads.publish(b, view_of(&repo_b, "beta", 100));

    let rows_a = reads.get(Some(a)).rows.as_ptr();
    let copies_before = reads.counters().copies;

    let _ = reads.get(Some(b)); // → B
    let back = reads.get(Some(a)); // → A
    let copies_after = reads.counters().copies;

    assert_eq!(back.rows.as_ptr(), rows_a, "the row vector was reallocated");
    assert_eq!(back.rows.len(), 3);
    assert_eq!(copies_before, copies_after, "a switch deep-copied a read");

    // Closing A releases its read; B is untouched.
    reads.forget(a);
    sessions.detach(a);
    assert!(reads.share(a).is_none());
    assert_eq!(reads.get(Some(b)).rows.len(), 4);
}

/// A failed read settles its slot, so the user can ask again — the failure never
/// sticks the tab on "Loading…" (the acceptance criterion #489 exists for).
#[test]
fn a_failed_read_can_be_re_requested() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = fixture(&root, "alpha", 2);
    let mut sessions = Sessions::new();
    let owner = attach(&mut sessions, &repo);
    let mut reads: Reads<TabViewState> = Reads::new();

    let failed = reads.begin(owner);
    assert!(reads.fail(failed));
    assert!(!reads.is_loading(owner));

    let retry = reads.begin(owner);
    assert!(reads.accept(retry, view_of(&repo, "alpha", 100)));
    assert_eq!(reads.get(Some(owner)).rows.len(), 2);
}

/// A mutation admitted against this worktree invalidates every read that
/// observed the repository before it, without blanking the display: the last
/// good rows stay on screen until the reload that follows lands.
///
/// Through the **real** admission path — `Sessions::write_lease` wrapped in
/// `admit`, exactly what `KagiApp::reserve_write` and `dispatch_job` call. The
/// first version of this test poked `Reads::invalidate` directly and therefore
/// could not see that nothing was wired to admission at all (stage-2 review,
/// item 1).
#[test]
fn an_admitted_write_refuses_the_read_that_predates_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = fixture(&root, "alpha", 2);
    let mut sessions = Sessions::new();
    let owner = attach(&mut sessions, &repo);
    let mut reads: Reads<TabViewState> = Reads::new();
    reads.publish(owner, view_of(&repo, "alpha", 100));

    // A read is in flight when the user starts a write.
    let in_flight = reads.begin(owner);
    let guard = admit(&mut reads, sessions.write_lease(&repo, LegacyBusy(false)))
        .expect("the lease is free");

    assert!(!reads.is_fresh(in_flight), "admission did not invalidate");
    assert!(!reads.accept(in_flight, view_of(&repo, "alpha", 100)));
    assert_eq!(
        reads.get(Some(owner)).rows.len(),
        2,
        "the last good read stayed on screen",
    );

    // The writer commits; the reload that follows is a fresh observation.
    std::fs::write(repo.join("f.txt"), "written\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "alpha written"]);
    guard.complete();

    let after = reads.begin(owner);
    assert!(reads.accept(after, view_of(&repo, "alpha", 100)));
    assert_eq!(reads.get(Some(owner)).rows.len(), 3);
}

/// A refused admission (the global busy lease is taken) invalidates nothing —
/// a write that never started cannot have changed what a reader observed.
#[test]
fn a_refused_write_leaves_the_read_in_flight_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = fixture(&root, "alpha", 2);
    let mut sessions = Sessions::new();
    let owner = attach(&mut sessions, &repo);
    let mut reads: Reads<TabViewState> = Reads::new();

    let held = sessions
        .write_lease(&repo, LegacyBusy(false))
        .expect("the first lease is free");
    let in_flight = reads.begin(owner);
    assert!(
        admit(&mut reads, sessions.write_lease(&repo, LegacyBusy(false))).is_err(),
        "a second lease must be refused",
    );
    assert!(
        reads.is_fresh(in_flight),
        "a refused write invalidated a read"
    );
    assert!(reads.accept(in_flight, view_of(&repo, "alpha", 100)));
    assert_eq!(reads.get(Some(owner)).rows.len(), 2);
    held.complete();
}

/// Paging refines the read on screen; it is not a fresh observation. A full
/// reload in flight — the watcher's, started by an external change — still
/// lands, because it is the one carrying the conflict re-detection and the
/// working-tree baseline that paging cannot reproduce (stage-2 review, item 3).
#[test]
fn load_more_does_not_supersede_a_pending_full_reload() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = fixture(&root, "alpha", 6);
    let mut sessions = Sessions::new();
    let owner = attach(&mut sessions, &repo);
    let mut reads: Reads<TabViewState> = Reads::new();
    reads.publish(owner, view_of(&repo, "alpha", 2));

    // The watcher's full reload starts, and reads the repository...
    let reload = reads.begin(owner);
    let reloaded = view_of(&repo, "alpha", 2);
    // ...while the user clicks "Load more", which lands first.
    reads.amend(owner, view_of(&repo, "alpha", 6));
    assert_eq!(
        reads.get(Some(owner)).rows.len(),
        6,
        "paging showed its page"
    );
    assert!(reads.is_loading(owner), "paging cancelled the reload");

    assert!(
        reads.accept(reload, reloaded),
        "the pending full reload was refused by paging",
    );
    assert_eq!(reads.get(Some(owner)).rows.len(), 2, "the reload landed");
}

/// A status-only refresh (the FS watcher's working-tree path) edits the owner's
/// read model in place: the commit rows and details it does not touch are not
/// copied, which is the whole reason a staging keystroke is cheap again.
#[test]
fn a_status_only_update_does_not_copy_the_commit_rows() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = fixture(&root, "alpha", 4);
    let mut sessions = Sessions::new();
    let owner = attach(&mut sessions, &repo);
    let mut reads: Reads<TabViewState> = Reads::new();
    reads.publish(owner, view_of(&repo, "alpha", 100));

    let rows = reads.get(Some(owner)).rows.as_ptr();
    let details = reads.get(Some(owner)).details.as_ptr();

    let view = reads.get_mut(Some(owner));
    view.status_summary.is_dirty = true;
    view.status_summary.unstaged = 1;
    view.is_dirty = true;

    let view = reads.get(Some(owner));
    assert!(view.is_dirty);
    assert_eq!(view.status_summary.unstaged, 1);
    assert_eq!(view.rows.as_ptr(), rows, "the commit rows were copied");
    assert_eq!(view.details.as_ptr(), details, "the details were copied");
}
