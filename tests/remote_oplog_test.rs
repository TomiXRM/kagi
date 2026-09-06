//! Exercise the remote command boundary with a local transport, not fake Git results.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

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

#[test]
fn remote_drop_persists_recovery_and_failed_attempt_before_returning() {
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
    // ssh normally hands its last argument to the remote login shell. This
    // transport does the same locally, against ONLY the throwaway repository.
    // Real git stash drop supplies both the exit status and recovery OID.
    let ssh = bin.join("ssh");
    std::fs::write(
        &ssh,
        "#!/bin/sh\nfor argument do command=$argument; done\nexec /bin/sh -c \"$command\"\n",
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
