//! #355: a reader's Skip makes the snapshot list upstreams with unknown
//! ahead/behind instead of counting them; a snapshot without Skip counts.
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, repo_with_bare_origin, write_file};

use git2::Repository;
use kagi_git::{snapshot_repairing_stat_cache, AheadBehind, SnapshotPhase, SnapshotProbe};

#[test]
fn skipped_ahead_behind_is_unknown_and_the_next_snapshot_counts_again() {
    let fixture = repo_with_bare_origin("main");
    write_file(&fixture.local, "b.txt", "b\n");
    commit_all(&fixture.local, "ahead by one");
    let mut repo = Repository::open(&fixture.local).expect("open");

    let skipped = SnapshotProbe::default();
    skipped.skip_ahead_behind();
    let snap = snapshot_repairing_stat_cache(&mut repo, 100, &skipped).expect("snapshot");
    let upstream = snap.branches[0].upstream.as_ref().expect("upstream kept");
    assert_eq!(upstream.remote_branch, "origin/main");
    assert_eq!(upstream.counts, None, "a skipped count is unknown, not 0/0");
    assert_eq!(
        skipped.phase(),
        SnapshotPhase::Other,
        "the phase ends with the read"
    );

    let counted =
        snapshot_repairing_stat_cache(&mut repo, 100, &SnapshotProbe::default()).expect("snapshot");
    assert_eq!(
        counted.branches[0].upstream.as_ref().unwrap().counts,
        Some(AheadBehind {
            ahead: 1,
            behind: 0
        })
    );
}
