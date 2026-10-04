//! Live SSH integration test for the remote read foundation (ADR-0089).
//!
//! Unlike the fake-SSH fixtures, these tests use the system SSH client and a
//! dedicated host. Both tests are `#[ignore]` so the default suite never
//! connects or silently reports a skipped live check as passed. An explicit
//! `--ignored` run requires the variables below and fails if they are absent.
//!
//! - `KAGI_REMOTE_TEST_HOST`: `[user@]host` with working authentication.
//! - `KAGI_REMOTE_TEST_PORT`: optional SSH port.
//! - `KAGI_REMOTE_TEST_REPO`: optional Git repository for read coverage.
//! - `KAGI_REMOTE_TEST_DIR`: optional non-repository directory for read coverage.
//! - `KAGI_REMOTE_TEST_PULL_REPO`: disposable checkout behind its upstream.
//! - `KAGI_REMOTE_TEST_PULL_EXPECTED_HEAD`: full upstream HEAD after pull.
//!
//! Run only the chosen live check against a disposable host, for example:
//! ```sh
//! KAGI_REMOTE_TEST_HOST=localhost KAGI_REMOTE_TEST_PORT=2222 \
//! KAGI_REMOTE_TEST_PULL_REPO=/tmp/remote-demo/clone \
//! KAGI_REMOTE_TEST_PULL_EXPECTED_HEAD="$(git -C /tmp/remote-demo/seed rev-parse HEAD)" \
//!   cargo test -p kagi --test remote_ssh_live_test live_remote_pull_fast_forwards -- --ignored --nocapture
//! ```

use kagi::remote;
use kagi_domain::remote::RemoteHost;

fn host_from_env() -> RemoteHost {
    let spec = std::env::var("KAGI_REMOTE_TEST_HOST")
        .expect("set KAGI_REMOTE_TEST_HOST for the explicitly selected live SSH test");
    let mut host = RemoteHost::parse(&spec).expect("KAGI_REMOTE_TEST_HOST should parse");
    if let Ok(port) = std::env::var("KAGI_REMOTE_TEST_PORT") {
        host.port = Some(port.parse().expect("KAGI_REMOTE_TEST_PORT should be a u16"));
    }
    host
}

#[test]
#[ignore = "requires an explicitly configured disposable SSH host"]
fn live_remote_read_path() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let host = host_from_env();
    eprintln!("== connecting to {} ==", host.label());

    // 1) Reachability + auth.
    remote::check_connection(&host).expect("check_connection should succeed");
    eprintln!("[ok] check_connection");

    // 2) Login (home) directory.
    let home = remote::home_dir(&host).expect("home_dir should succeed");
    eprintln!("[ok] home_dir = {home}");
    assert!(home.starts_with('/'), "home should be an absolute path");

    // 3) Directory listing of $HOME.
    let entries = remote::list_dir(&host, &home).expect("list_dir should succeed");
    eprintln!("[ok] list_dir({home}) -> {} entries", entries.len());

    // 4) Repository detection — positive case.
    if let Ok(repo_path) = std::env::var("KAGI_REMOTE_TEST_REPO") {
        let probe = remote::probe_repo(&host, &repo_path).expect("probe_repo should succeed");
        eprintln!("[ok] probe_repo({repo_path}) -> {probe:?}");
        assert!(probe.is_repo, "{repo_path} should be detected as a repo");
        assert!(probe.toplevel.is_some(), "a repo should report a toplevel");

        let summary = remote::repo_summary(&host, &repo_path)
            .expect("repo_summary should succeed")
            .expect("a repo with commits should have a HEAD summary");
        eprintln!("[ok] repo_summary -> {summary:?}");
        assert!(!summary.head_short.is_empty(), "HEAD short hash present");
        assert!(!summary.summary.is_empty(), "HEAD subject present");

        // Phase 2: a full RepoSnapshot over SSH (same type the local git2
        // backend produces).
        let snap = remote::remote_snapshot(&host, &repo_path, 1000)
            .expect("remote_snapshot should succeed");
        eprintln!(
            "[ok] remote_snapshot -> {} commits, {} branches, {} remote, {} tags, head={:?}",
            snap.commits.len(),
            snap.branches.len(),
            snap.remote_branches.len(),
            snap.tags.len(),
            snap.head,
        );
        assert!(!snap.commits.is_empty(), "repo with commits has commits");
        assert!(
            !snap.branches.is_empty(),
            "repo has at least one local branch"
        );
        // The newest commit's subject matches the one-line summary.
        assert_eq!(snap.commits[0].summary, summary.summary);
        // Parent links are populated (single-parent for a linear history).
        assert!(
            snap.commits.iter().any(|c| c.parents.len() <= 1),
            "linear history parses parents"
        );

        // Phase 2c: changed files + unified file diff over SSH for a normal
        // single-parent commit (merges have special first-parent diffs).
        if let Some(commit) = snap.commits.iter().find(|c| c.parents.len() == 1) {
            let sha = &commit.id.0;
            let files = remote::remote_commit_changed_files(&host, &repo_path, sha)
                .expect("remote_commit_changed_files should succeed");
            eprintln!("[ok] changed_files({}) -> {} files", &sha[..8], files.len());
            assert!(
                !files.is_empty(),
                "a non-merge content commit changes files"
            );

            let path = files[0].path.to_string_lossy().into_owned();
            let diff = remote::remote_commit_file_diff(&host, &repo_path, sha, &path)
                .expect("remote_commit_file_diff should succeed");
            let lines: usize = diff.hunks.iter().map(|h| h.lines.len()).sum();
            eprintln!(
                "[ok] file_diff({path}) -> {} hunks, {} lines, binary={}",
                diff.hunks.len(),
                lines,
                diff.is_binary
            );
            assert!(
                diff.is_binary || !diff.hunks.is_empty(),
                "text diff has hunks"
            );
        }

        // Working-tree status parses (clean or dirty — just must not error).
        eprintln!(
            "[ok] status -> staged={} unstaged={} untracked={} conflicted={}",
            snap.status.staged.len(),
            snap.status.unstaged.len(),
            snap.status.untracked.len(),
            snap.status.conflicted.len(),
        );
    }

    // 5) Repository detection — negative case (a non-repo dir is not an error).
    if let Ok(dir) = std::env::var("KAGI_REMOTE_TEST_DIR") {
        let probe =
            remote::probe_repo(&host, &dir).expect("probe_repo should not error on non-repo");
        eprintln!("[ok] probe_repo({dir}) -> {probe:?}");
        assert!(!probe.is_repo, "{dir} should NOT be detected as a repo");
    }

    eprintln!("== all live remote checks passed ==");
}

/// Opt in only with a disposable checkout: unlike the read probe, this writes
/// the remote repository after the approved preflight succeeds.
#[test]
#[ignore = "requires a disposable checkout with an approved upstream"]
fn live_remote_pull_fast_forwards() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let host = host_from_env();
    let repo_path = std::env::var("KAGI_REMOTE_TEST_PULL_REPO")
        .expect("set KAGI_REMOTE_TEST_PULL_REPO for the selected live SSH pull");
    let expected_head = std::env::var("KAGI_REMOTE_TEST_PULL_EXPECTED_HEAD")
        .expect("set KAGI_REMOTE_TEST_PULL_EXPECTED_HEAD for the selected live SSH pull");
    let frozen =
        remote::resolve_pull_identity(&host, &repo_path).expect("live pull plan should resolve");
    assert_ne!(frozen.head.oid, expected_head, "fixture must start behind");
    let before = kagi_git::StateSummary {
        head: frozen.head.oid.clone(),
        dirty: "clean".into(),
    };
    // Even on a real SSH transport, the approved command must refuse before
    // Git executes if the frozen HEAD is no longer the host's HEAD.
    let mut stale = frozen.clone();
    stale.head.oid = "0".repeat(40);
    let refused = remote::remote_pull(&host, &repo_path, &stale, &before);
    assert!(matches!(
        refused.recording.entry().outcome,
        kagi_git::oplog::OpOutcome::Refused { ref blockers }
            if blockers.as_slice() == ["HEAD changed"]
    ));
    assert_eq!(
        remote::resolve_pull_identity(&host, &repo_path)
            .expect("refusal must preserve the live checkout")
            .head
            .oid,
        frozen.head.oid
    );
    let report = remote::remote_pull(&host, &repo_path, &frozen, &before);
    report
        .result
        .expect("approved live SSH pull should succeed");
    let after = remote::resolve_pull_identity(&host, &repo_path)
        .expect("live pull result should be readable");
    assert_eq!(
        after.head.oid, expected_head,
        "host checkout must fast-forward"
    );
    eprintln!("[ok] live remote pull fast-forwarded over system SSH");
}

#[path = "support/isolated.rs"]
mod test_support;
