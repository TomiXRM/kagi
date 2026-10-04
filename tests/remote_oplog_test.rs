//! Exercise the remote command boundary with a local transport, not fake Git results.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

#[path = "support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output as git;

/// PATH and KAGI_LOG_DIR are process-global; the tests here share them.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct Environment {
    path: Option<OsString>,
    log: Option<OsString>,
}

impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in [("PATH", &self.path), ("KAGI_LOG_DIR", &self.log)] {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

/// The ssh stand-in: ssh hands its last argument to the remote login shell, and
/// so does this — against ONLY the throwaway repository the caller just built.
fn fake_ssh(bin: &Path) {
    let ssh = bin.join("ssh");
    std::fs::write(
        &ssh,
        "#!/bin/sh\nif [ \"$1\" = -G ]; then\n  printf 'hostname fixture.invalid\\nuser alice\\nport 22\\nidentityfile none\\nuserknownhostsfile none\\nglobalknownhostsfile none\\nproxyjump none\\n'\n  exit 0\nfi\nfor argument do command=$argument; done\nexec /bin/sh -c \"$command\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn pull_probe_binds_symlink_to_real_worktree_and_live_head() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let alias = root.path().join("selected");
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "fixture@example.invalid"]);
    git(&repo, &["config", "user.name", "fixture"]);
    std::fs::write(repo.join("file"), "base\n").unwrap();
    git(&repo, &["add", "file"]);
    git(&repo, &["commit", "-qm", "base"]);
    git(
        &repo,
        &["config", "remote.origin.url", "ssh://fixture.invalid/repo"],
    );
    git(
        &repo,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(&repo, &["branch", "--set-upstream-to=origin/main", "main"]);
    std::os::unix::fs::symlink(&repo, &alias).unwrap();
    fake_ssh(&bin);
    let _restore = Environment {
        path: std::env::var_os("PATH"),
        log: std::env::var_os("KAGI_LOG_DIR"),
    };
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &_restore.path.clone().unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    let host = kagi_domain::remote::RemoteHost::parse("fixture.invalid").unwrap();
    let planned = kagi::remote::resolve_pull_identity(&host, alias.to_str().unwrap()).unwrap();
    assert_eq!(
        planned.repo_id.common_dir,
        repo.join(".git").canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(
        planned.physical_toplevel,
        repo.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(planned.head.branch.as_deref(), Some("main"));
    assert_eq!(planned.head.oid, git(&repo, &["rev-parse", "HEAD"]));
    assert_eq!(planned.head.upstream.as_deref(), Some("origin/main"));
    let config = planned.config.as_ref().unwrap();
    assert_eq!(config.remote_name, "origin");
    assert_eq!(config.merge_ref, "refs/heads/main");
    assert_eq!(config.remote_url, "ssh://fixture.invalid/repo");
    assert_eq!(
        config.fetch_refspecs,
        ["+refs/heads/*:refs/remotes/origin/*"]
    );
    git(&repo, &["checkout", "-qb", "feature"]);
    git(
        &repo,
        &["branch", "--set-upstream-to=origin/main", "feature"],
    );
    let observed = kagi::remote::resolve_pull_identity(&host, alias.to_str().unwrap()).unwrap();
    assert_eq!(planned.repo_id, observed.repo_id);
    assert_eq!(planned.physical_toplevel, observed.physical_toplevel);
    assert_ne!(planned.head, observed.head);
    assert_eq!(observed.head.branch.as_deref(), Some("feature"));
    git(
        &repo,
        &[
            "config",
            "remote.origin.url",
            "ssh://fixture.invalid/changed",
        ],
    );
    let changed = kagi::remote::resolve_pull_identity(&host, alias.to_str().unwrap()).unwrap();
    assert_eq!(observed.head, changed.head);
    assert_ne!(observed.config, changed.config);
    git(
        &repo,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/main:refs/remotes/origin/main",
        ],
    );
    let changed_fetch =
        kagi::remote::resolve_pull_identity(&host, alias.to_str().unwrap()).unwrap();
    assert_eq!(changed.head, changed_fetch.head);
    assert_ne!(changed.config, changed_fetch.config);
    git(&repo, &["config", "--unset", "remote.origin.fetch"]);
    assert!(
        kagi::remote::resolve_pull_identity(&host, alias.to_str().unwrap()).is_err(),
        "missing fetch configuration must fail closed"
    );
}

#[test]
fn remote_drop_persists_recovery_and_failed_attempt_before_returning() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo with spaces");
    let bin = root.path().join("bin");
    let logs = root.path().join("logs");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    git(&repo, &["init", "-q", "-b", "main", "--object-format=sha1"]);
    git(&repo, &["config", "core.hooksPath", "/dev/null"]);
    git(&repo, &["config", "commit.gpgSign", "false"]);
    std::fs::write(repo.join("file"), "base\n").unwrap();
    git(&repo, &["add", "file"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    std::fs::write(repo.join("file"), "recover this remote work\n").unwrap();
    git(&repo, &["stash", "push", "-q"]);
    let expected = git(&repo, &["rev-parse", "refs/stash"]);
    let before = kagi_git::StateSummary {
        head: git(&repo, &["rev-parse", "HEAD"]),
        dirty: "clean".into(),
    };
    // Real git stash drop supplies both the exit status and recovery OID.
    fake_ssh(&bin);
    let _restore = Environment {
        path: std::env::var_os("PATH"),
        log: std::env::var_os("KAGI_LOG_DIR"),
    };
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &_restore.path.clone().unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    std::env::set_var("KAGI_LOG_DIR", &logs);
    let host = kagi_domain::remote::RemoteHost::parse("fixture.invalid").unwrap();
    let report = kagi::remote::remote_stash_drop(&host, repo.to_str().unwrap(), 0, &before);
    report.result.expect("the drop itself must succeed");
    // #643 A1: the receipt comes back with the result, so a record that never
    // landed cannot look like one that did.
    assert!(
        matches!(
            report.recording,
            kagi_git::backend::recording::Recording::Appended { .. }
        ),
        "the attempt must be recorded, and the caller must be able to see that \
         it was: {:?}",
        report.recording
    );
    assert!(git(&repo, &["stash", "list"]).is_empty());
    let entries = kagi_git::oplog::read_oplog_tail(10);
    assert_eq!(entries.len(), 1);
    let kagi_git::oplog::OpOutcome::Success { after } = &entries[0].outcome else {
        panic!("drop outcome was not success");
    };
    let logged = after
        .dirty
        .split(|c: char| !c.is_ascii_hexdigit())
        .find(|value| value.len() == 40)
        .expect("remote result lost recovery OID");
    assert_eq!(logged, expected);
    assert_eq!(
        entries[0].repo,
        format!("fixture.invalid:{}", repo.display())
    );
    let failed = kagi::remote::remote_stash_drop(&host, repo.to_str().unwrap(), 0, &before);
    assert!(failed.result.is_err());
    assert!(
        matches!(
            failed.recording,
            kagi_git::backend::recording::Recording::Appended { .. }
        ),
        "a failed drop is still an attempt, and the attempt is still recorded: {:?}",
        failed.recording
    );
    let entries = kagi_git::oplog::read_oplog_tail(10);
    assert_eq!(entries.len(), 2);
    assert!(matches!(
        entries[0].outcome,
        kagi_git::oplog::OpOutcome::Failed { .. }
    ));
    git(&repo, &["stash", "store", "-m", "recovered", logged]);
    assert_eq!(
        git(&repo, &["show", "refs/stash:file"]),
        "recover this remote work"
    );
}

/// #501: the SSH pull used to be recorded nowhere — the UI called the
/// presentation-only `record_op`, and the transport appended nothing. The
/// record must exist without any UI completion running.
#[test]
fn remote_pull_records_success_and_failure_at_the_transport() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let origin = root.path().join("origin.git");
    let repo = root.path().join("clone");
    let seed = root.path().join("seed");
    let bin = root.path().join("bin");
    let logs = root.path().join("logs");
    for dir in [&origin, &seed, &bin] {
        std::fs::create_dir_all(dir).unwrap();
    }
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    git(&seed, &["init", "-q", "-b", "main"]);
    git(&seed, &["config", "core.hooksPath", "/dev/null"]);
    git(&seed, &["config", "commit.gpgSign", "false"]);
    std::fs::write(seed.join("file"), "base\n").unwrap();
    git(&seed, &["add", "file"]);
    git(&seed, &["commit", "-q", "-m", "base"]);
    git(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
    git(
        root.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            repo.to_str().unwrap(),
        ],
    );

    let before = kagi_git::StateSummary {
        head: git(&repo, &["rev-parse", "HEAD"]),
        dirty: "clean".into(),
    };
    fake_ssh(&bin);
    let _restore = Environment {
        path: std::env::var_os("PATH"),
        log: std::env::var_os("KAGI_LOG_DIR"),
    };
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &_restore.path.clone().unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    std::env::set_var("KAGI_LOG_DIR", &logs);
    let host = kagi_domain::remote::RemoteHost::parse("fixture.invalid").unwrap();

    let report = kagi::remote::remote_pull(
        &host,
        repo.to_str().unwrap(),
        repo.to_str().unwrap(),
        &before,
    );
    let summary = report.result.expect("fixture pull should succeed");
    assert!(
        matches!(
            report.recording,
            kagi_git::backend::recording::Recording::Appended { .. }
        ),
        "the transport must hand back its receipt, not swallow the append"
    );
    let entries = kagi_git::oplog::read_oplog_tail(10);
    assert_eq!(entries.len(), 1, "a remote pull must be recorded");
    assert_eq!(entries[0].op, "pull");
    let scope = format!("fixture.invalid:{}", repo.display());
    assert_eq!(entries[0].repo, scope);
    assert_eq!(entries[0].worktree.as_deref(), Some(scope.as_str()));
    let kagi_git::oplog::OpOutcome::Success { after } = &entries[0].outcome else {
        panic!("pull outcome was not success");
    };
    assert_eq!(after.dirty, summary);

    // A repository that is not there is an explicit refusal — git declined
    // before touching anything, so Failed is provable.
    let missing = root.path().join("gone");
    let report = kagi::remote::remote_pull(
        &host,
        missing.to_str().unwrap(),
        missing.to_str().unwrap(),
        &before,
    );
    assert!(report.result.is_err());
    let entries = kagi_git::oplog::read_oplog_tail(10);
    assert_eq!(entries.len(), 2);
    assert!(matches!(
        entries[0].outcome,
        kagi_git::oplog::OpOutcome::Failed { .. }
    ));

    // A pull that stopped mid-merge changed the host: Partial, not Failed.
    git(&seed, &["pull", "-q", origin.to_str().unwrap(), "main"]);
    std::fs::write(seed.join("file"), "theirs\n").unwrap();
    git(&seed, &["commit", "-qam", "theirs"]);
    git(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
    git(&repo, &["config", "pull.rebase", "false"]);
    git(&repo, &["config", "user.email", "fixture@example.invalid"]);
    git(&repo, &["config", "user.name", "fixture"]);
    std::fs::write(repo.join("file"), "ours\n").unwrap();
    git(&repo, &["commit", "-qam", "ours"]);
    let report = kagi::remote::remote_pull(
        &host,
        repo.to_str().unwrap(),
        repo.to_str().unwrap(),
        &before,
    );
    assert!(report.result.is_err());
    let entries = kagi_git::oplog::read_oplog_tail(10);
    assert_eq!(entries.len(), 3);
    let kagi_git::oplog::OpOutcome::Partial { after, .. } = &entries[0].outcome else {
        panic!(
            "a conflicted pull must be partial, got {:?}",
            entries[0].outcome
        );
    };
    assert!(after.dirty.contains("mid-merge"), "{}", after.dirty);
    assert!(!git(&repo, &["status", "--porcelain"]).is_empty());
}

/// Run a remote pull against an `ssh` stand-in that just prints `stderr_line`
/// and exits non-zero, and return the single entry it recorded.
fn pull_outcome_for_ssh_output(stderr_line: &str) -> kagi_git::oplog::OpOutcome {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    let logs = root.path().join("logs");
    std::fs::create_dir_all(&bin).unwrap();
    let ssh = bin.join("ssh");
    std::fs::write(
        &ssh,
        format!("#!/bin/sh\necho '{stderr_line}' >&2\nexit 255\n"),
    )
    .unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let _restore = Environment {
        path: std::env::var_os("PATH"),
        log: std::env::var_os("KAGI_LOG_DIR"),
    };
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &_restore.path.clone().unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    std::env::set_var("KAGI_LOG_DIR", &logs);
    let host = kagi_domain::remote::RemoteHost::parse("fixture.invalid").unwrap();
    let before = kagi_git::StateSummary {
        head: "main".into(),
        dirty: "clean".into(),
    };

    let report = kagi::remote::remote_pull(&host, "/srv/repo", "/srv/repo", &before);
    assert!(report.result.is_err());
    let entries = kagi_git::oplog::read_oplog_tail(10);
    assert_eq!(entries.len(), 1);
    entries[0].outcome.clone()
}

/// #501 (PM ruling): `Failed` needs a recognized refusal. A lost session and an
/// output nobody recognizes both default to `Unknown` — a false Unknown holds
/// the lease and asks; a false Failed invites a retry against a host whose
/// `git pull` may already have run.
#[test]
fn a_non_zero_remote_pull_is_unknown_unless_the_refusal_is_recognized() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    for banner in [
        // ssh's own disconnect banner.
        "Connection closed by 10.0.0.1 port 22",
        // Nothing in the allow-list: the default must still be Unknown.
        "remote: something nobody has taught kagi to read",
    ] {
        let outcome = pull_outcome_for_ssh_output(banner);
        let kagi_git::oplog::OpOutcome::Unknown { evidence, .. } = &outcome else {
            panic!("{banner:?} must be Unknown, got {outcome:?}");
        };
        assert!(evidence.contains("do not retry"), "{evidence}");
    }

    // Only a recognized refusal proves nothing ran.
    let outcome = pull_outcome_for_ssh_output("Permission denied (publickey).");
    assert!(
        matches!(outcome, kagi_git::oplog::OpOutcome::Failed { .. }),
        "a recognized refusal must stay Failed, got {outcome:?}"
    );
}

#[path = "support/isolated.rs"]
mod test_support;

/// #643 A1: when the oplog append fails, the caller can tell.
///
/// The point of returning a receipt is that a record which never landed stops
/// looking like one that did. Before this, the append error was discarded with
/// `let _` and the drop reported plain success.
#[test]
fn a_remote_drop_whose_record_cannot_be_written_says_so() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // PATH and KAGI_LOG_DIR are process-global; share them like the tests above.
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let logs = tmp.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    fake_ssh(&bin);
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("file"), "work\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "seed"]);
    std::fs::write(repo.join("file"), "changed\n").unwrap();
    git(&repo, &["stash", "push", "-qm", "wip"]);

    let _restore = Environment {
        path: std::env::var_os("PATH"),
        log: std::env::var_os("KAGI_LOG_DIR"),
    };
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &_restore.path.clone().unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    std::env::set_var("KAGI_LOG_DIR", &logs);

    // Hold the oplog's lock so the append cannot take it.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(logs.join("operations.jsonl.lock"))
        .unwrap();
    lock.lock().unwrap();

    let before = kagi_git::StateSummary {
        head: git(&repo, &["rev-parse", "HEAD"]),
        dirty: "clean".into(),
    };
    let host = kagi_domain::remote::RemoteHost::parse("fixture.invalid").unwrap();
    let report = kagi::remote::remote_stash_drop(&host, repo.to_str().unwrap(), 0, &before);

    assert!(
        matches!(
            report.recording,
            kagi_git::backend::recording::Recording::Failed { .. }
        ),
        "the caller must be able to see that the record did not land: {:?}",
        report.recording
    );

    drop(lock);
}
