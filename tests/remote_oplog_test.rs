//! Exercise the remote command boundary with a local transport, not fake Git results.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

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

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {:?}", output.stderr);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// The ssh stand-in: ssh hands its last argument to the remote login shell, and
/// so does this — against ONLY the throwaway repository the caller just built.
fn fake_ssh(bin: &Path) {
    let ssh = bin.join("ssh");
    std::fs::write(
        &ssh,
        "#!/bin/sh\nfor argument do command=$argument; done\nexec /bin/sh -c \"$command\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn remote_drop_persists_recovery_and_failed_attempt_before_returning() {
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
    kagi::remote::remote_stash_drop(&host, repo.to_str().unwrap(), 0, &before).unwrap();
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
    assert!(kagi::remote::remote_stash_drop(&host, repo.to_str().unwrap(), 0, &before).is_err());
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

    let summary = kagi::remote::remote_pull(&host, repo.to_str().unwrap(), &before).unwrap();
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

    // A repository that is not there is still an accepted attempt.
    let missing = root.path().join("gone");
    assert!(kagi::remote::remote_pull(&host, missing.to_str().unwrap(), &before).is_err());
    let entries = kagi_git::oplog::read_oplog_tail(10);
    assert_eq!(entries.len(), 2);
    assert!(matches!(
        entries[0].outcome,
        kagi_git::oplog::OpOutcome::Failed { .. }
    ));
}
