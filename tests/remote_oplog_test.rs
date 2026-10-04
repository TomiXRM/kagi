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
fn pull_probe_and_single_session_preflight_bind_the_approved_worktree() {
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
    // Observe the real probe's Git subprocesses, not its script text: status
    // must not take an optional index lock during planning.
    let pull_attempts = root.path().join("pull-attempts");
    let git_guard = bin.join("git");
    std::fs::write(
        &git_guard,
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = status ] && [ \"$GIT_OPTIONAL_LOCKS\" != 0 ]; then echo 'status may lock index' >&2; exit 81; fi\n\
             case \" $* \" in *' pull '*) printf 'pull\\n' >> {} ;; esac\n\
             exec /usr/bin/git \"$@\"\n",
            kagi_domain::remote::shell_quote(pull_attempts.to_str().unwrap()),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&git_guard, std::fs::Permissions::from_mode(0o700)).unwrap();
    let scratch_attempts = root.path().join("scratch-attempts");
    let mktemp_guard = bin.join("mktemp");
    std::fs::write(
        &mktemp_guard,
        format!(
            "#!/bin/sh\nprintf 'attempt\\n' >> {}\nexec /usr/bin/mktemp \"$@\"\n",
            kagi_domain::remote::shell_quote(scratch_attempts.to_str().unwrap()),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&mktemp_guard, std::fs::Permissions::from_mode(0o700)).unwrap();
    let _restore = Environment {
        path: std::env::var_os("PATH"),
        log: std::env::var_os("KAGI_LOG_DIR"),
    };
    let mut paths = vec![bin.clone()];
    paths.extend(std::env::split_paths(
        &_restore.path.clone().unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    std::env::set_var("KAGI_LOG_DIR", root.path().join("logs"));
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
    let before = kagi_git::StateSummary {
        head: planned.head.oid.clone(),
        dirty: "clean".into(),
    };
    // Scratch files inside either part of the repository are writes. Refuse
    // before invoking mktemp (and before invoking pull).
    let original_tmpdir = std::env::var_os("TMPDIR");
    let git_dir = repo.join(".git");
    for scratch_root in [&repo, &git_dir] {
        std::env::set_var("TMPDIR", scratch_root);
        let refusal = kagi::remote::remote_pull(&host, alias.to_str().unwrap(), &planned, &before);
        assert!(matches!(
            refusal.recording.entry().outcome,
            kagi_git::oplog::OpOutcome::Refused { ref blockers }
                if blockers.iter().any(|reason| reason.contains("scratch directory is inside"))
        ));
        assert!(
            !scratch_attempts.exists(),
            "no scratch file may be created in the repository"
        );
    }
    if let Some(tmpdir) = original_tmpdir {
        std::env::set_var("TMPDIR", tmpdir);
    } else {
        std::env::remove_var("TMPDIR");
    }
    assert!(
        !pull_attempts.exists(),
        "rejected scratch location cannot reach git pull"
    );
    let status = std::process::Command::new("/usr/bin/git")
        .arg("-C")
        .arg(&repo)
        .args(["status", "--porcelain"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(
        status.stdout.is_empty(),
        "preflight must not dirty the worktree"
    );
    std::fs::write(repo.join("file"), "unstaged change\n").unwrap();
    let dirty = kagi::remote::resolve_pull_identity(&host, alias.to_str().unwrap()).unwrap();
    assert_eq!(planned.head, dirty.head);
    assert_eq!(planned.config, dirty.config);
    assert_eq!(planned.fingerprint.staged, dirty.fingerprint.staged);
    assert_ne!(planned.fingerprint.worktree, dirty.fingerprint.worktree);
    let refusal = kagi::remote::remote_pull(&host, alias.to_str().unwrap(), &planned, &before);
    assert!(matches!(
        refusal.recording.entry().outcome,
        kagi_git::oplog::OpOutcome::Refused { ref blockers }
            if blockers.iter().any(|reason| reason.contains("worktree status changed"))
    ));
    git(&repo, &["add", "file"]);
    let staged = kagi::remote::resolve_pull_identity(&host, alias.to_str().unwrap()).unwrap();
    assert_eq!(planned.head, staged.head);
    assert_eq!(planned.config, staged.config);
    assert_ne!(planned.fingerprint.staged, staged.fingerprint.staged);
    assert_ne!(dirty.fingerprint.worktree, staged.fingerprint.worktree);
    let refusal = kagi::remote::remote_pull(&host, alias.to_str().unwrap(), &planned, &before);
    assert!(matches!(
        refusal.recording.entry().outcome,
        kagi_git::oplog::OpOutcome::Refused { ref blockers }
            if blockers.iter().any(|reason| reason.contains("staged index changed"))
    ));
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
    let refusal = kagi::remote::remote_pull(&host, alias.to_str().unwrap(), &planned, &before);
    assert!(matches!(
        refusal.recording.entry().outcome,
        kagi_git::oplog::OpOutcome::Refused { ref blockers }
            if blockers.iter().any(|reason| reason.contains("branch changed"))
    ));
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
    let refusal = kagi::remote::remote_pull(&host, alias.to_str().unwrap(), &observed, &before);
    assert!(matches!(
        refusal.recording.entry().outcome,
        kagi_git::oplog::OpOutcome::Refused { ref blockers }
            if blockers.iter().any(|reason| reason.contains("remote URL changed"))
    ));
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
    let refusal = kagi::remote::remote_pull(&host, alias.to_str().unwrap(), &changed, &before);
    assert!(matches!(
        refusal.recording.entry().outcome,
        kagi_git::oplog::OpOutcome::Refused { ref blockers }
            if blockers.iter().any(|reason| reason.contains("fetch refspecs changed"))
    ));
    git(&repo, &["config", "--unset", "remote.origin.fetch"]);
    assert!(
        kagi::remote::resolve_pull_identity(&host, alias.to_str().unwrap()).is_err(),
        "missing fetch configuration must fail closed"
    );
    let other = root.path().join("other");
    git(
        root.path(),
        &[
            "clone",
            "-q",
            repo.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&other, &alias).unwrap();
    let refusal = kagi::remote::remote_pull(&host, alias.to_str().unwrap(), &planned, &before);
    assert!(matches!(
        refusal.recording.entry().outcome,
        kagi_git::oplog::OpOutcome::Refused { ref blockers }
            if blockers.iter().any(|reason| reason.contains("repository changed"))
    ));
    let ssh = bin.join("ssh");
    let changed_route = std::fs::read_to_string(&ssh)
        .unwrap()
        .replace("hostname fixture.invalid", "hostname changed.invalid");
    std::fs::write(&ssh, changed_route).unwrap();
    let refusal = kagi::remote::remote_pull(&host, alias.to_str().unwrap(), &planned, &before);
    assert!(matches!(
        refusal.recording.entry().outcome,
        kagi_git::oplog::OpOutcome::Refused { ref blockers }
            if blockers.iter().any(|reason| reason.contains("SSH connection changed"))
    ));
    let entries = kagi_git::oplog::read_oplog_tail(10);
    assert_eq!(
        entries.len(),
        9,
        "each host-side refusal has one durable receipt"
    );
    assert!(entries
        .iter()
        .all(|entry| matches!(entry.outcome, kagi_git::oplog::OpOutcome::Refused { .. })));
    assert!(!pull_attempts.exists(), "no drift may reach git pull");
}

/// #1014: after the confirmation, another SSH connection must not open between
/// the live preflight and pull. The stand-in redirects origin immediately before
/// a *third* remote connection; the old two-connection execution pulls from it.
#[test]
fn remote_pull_cannot_switch_origin_between_preflight_and_pull_connections() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let origin = root.path().join("origin.git");
    let attacker = root.path().join("other.git");
    let seed = root.path().join("seed");
    let repo = root.path().join("clone");
    let bin = root.path().join("bin");
    let calls = root.path().join("ssh-connections");
    let logs = root.path().join("logs");
    for dir in [&origin, &seed, &bin] {
        std::fs::create_dir_all(dir).unwrap();
    }
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    git(&seed, &["init", "-q", "-b", "main"]);
    git(&seed, &["config", "user.email", "fixture@example.invalid"]);
    git(&seed, &["config", "user.name", "fixture"]);
    std::fs::write(seed.join("file"), "base\n").unwrap();
    git(&seed, &["add", "file"]);
    git(&seed, &["commit", "-qm", "base"]);
    git(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
    git(
        root.path(),
        &[
            "clone",
            "-q",
            "--bare",
            origin.to_str().unwrap(),
            attacker.to_str().unwrap(),
        ],
    );
    git(
        root.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            repo.to_str().unwrap(),
        ],
    );
    std::fs::write(seed.join("file"), "unapproved remote commit\n").unwrap();
    git(&seed, &["commit", "-qam", "other"]);
    git(&seed, &["push", "-q", attacker.to_str().unwrap(), "main"]);
    let approved_head = git(&repo, &["rev-parse", "HEAD"]);

    let quoted_calls = kagi_domain::remote::shell_quote(calls.to_str().unwrap());
    let quoted_repo = kagi_domain::remote::shell_quote(repo.to_str().unwrap());
    let quoted_attacker = kagi_domain::remote::shell_quote(attacker.to_str().unwrap());
    let ssh = bin.join("ssh");
    std::fs::write(
        &ssh,
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = -G ]; then\n\
             printf 'hostname fixture.invalid\\nuser alice\\nport 22\\nidentityfile none\\nuserknownhostsfile none\\nglobalknownhostsfile none\\nproxyjump none\\n'\n\
             exit 0\n\
             fi\n\
             printf 'connection\\n' >> {quoted_calls}\n\
             if [ \"$(wc -l < {quoted_calls})\" -eq 3 ]; then\n\
             /usr/bin/git -C {quoted_repo} config remote.origin.url {quoted_attacker}\n\
             fi\n\
             for argument do command=$argument; done\n\
             exec /bin/sh -c \"$command\"\n"
        ),
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
    let mut sessions = kagi::app::Sessions::new();
    let session = sessions.attach(format!("fixture.invalid:{}", repo.display()).into());
    let request = kagi::app::RemotePullRequest {
        owner: kagi::remote::stash::RemoteAttachment {
            session,
            host,
            root: repo.to_str().unwrap().into(),
        },
        plan: std::sync::Arc::new(kagi_git::plan_pull_remote(
            "main",
            "origin/main",
            1,
            0,
            false,
            approved_head.clone(),
        )),
        cached_head_oid: Some(approved_head.clone()),
        cached_remote_dirty: false,
    };
    let planned = kagi::app::plan_remote_pull(&mut sessions, request).run();
    assert!(kagi::app::apply_plan(&mut sessions, planned));
    let token = match sessions.plan_state() {
        kagi::app::PlanState::Ready { token, .. } => token.clone(),
        _ => panic!("fixture plan should be ready"),
    };
    let approved = kagi::app::approve(
        &mut sessions,
        token,
        kagi::app::Policy::Stash(Default::default()),
    )
    .unwrap();
    let kagi::app::Job::RemotePull(job) = kagi::app::prepare(&mut sessions, approved).unwrap()
    else {
        panic!("fixture must prepare a remote pull");
    };
    let completion = job.run();
    assert!(
        completion.report.result.is_ok(),
        "{:?}",
        completion.report.result
    );
    kagi::app::apply(&mut sessions, completion);
    assert_eq!(
        git(&repo, &["rev-parse", "HEAD"]),
        approved_head,
        "an unapproved repository selected by a second SSH connection was pulled"
    );
    assert_eq!(
        std::fs::read_to_string(&calls).unwrap().lines().count(),
        2,
        "planning and execution each open one SSH session, not preflight plus pull"
    );
    assert_eq!(
        git(&repo, &["config", "--get", "remote.origin.url"]),
        origin.to_str().unwrap()
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

    let frozen = kagi::remote::resolve_pull_identity(&host, repo.to_str().unwrap()).unwrap();
    let report = kagi::remote::remote_pull(&host, repo.to_str().unwrap(), &frozen, &before);
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

    // A confirmed repository that has since disappeared is a durable
    // pre-exec Refused; no Git command can be run there.
    let missing = root.path().join("gone");
    let report = kagi::remote::remote_pull(&host, missing.to_str().unwrap(), &frozen, &before);
    assert!(report.result.is_err());
    let entries = kagi_git::oplog::read_oplog_tail(10);
    assert_eq!(entries.len(), 2);
    assert!(matches!(
        entries[0].outcome,
        kagi_git::oplog::OpOutcome::Refused { .. }
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
    let frozen = kagi::remote::resolve_pull_identity(&host, repo.to_str().unwrap()).unwrap();
    let report = kagi::remote::remote_pull(&host, repo.to_str().unwrap(), &frozen, &before);
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

/// A confirmation promises a merge pull, not a rebase or ff-only refusal
/// chosen later by the remote worktree's mutable Git configuration.
#[test]
fn remote_pull_merges_even_when_host_config_requests_rebase_and_ff_only() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let origin = root.path().join("origin.git");
    let seed = root.path().join("seed");
    let repo = root.path().join("clone");
    let bin = root.path().join("bin");
    let logs = root.path().join("logs");
    for dir in [&origin, &seed, &bin] {
        std::fs::create_dir_all(dir).unwrap();
    }
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    git(&seed, &["init", "-q", "-b", "main"]);
    std::fs::write(seed.join("base"), "base\n").unwrap();
    git(&seed, &["add", "base"]);
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
    std::fs::write(seed.join("upstream"), "upstream\n").unwrap();
    git(&seed, &["add", "upstream"]);
    git(&seed, &["commit", "-q", "-m", "upstream"]);
    let upstream_oid = git(&seed, &["rev-parse", "HEAD"]);
    git(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
    std::fs::write(repo.join("local"), "local\n").unwrap();
    git(&repo, &["add", "local"]);
    git(&repo, &["commit", "-q", "-m", "local"]);
    let local_oid = git(&repo, &["rev-parse", "HEAD"]);
    for (key, value) in [
        ("pull.rebase", "true"),
        ("branch.main.rebase", "true"),
        ("pull.ff", "only"),
        ("branch.main.mergeOptions", "--squash"),
        ("user.email", "fixture@example.invalid"),
        ("user.name", "fixture"),
        ("commit.gpgSign", "false"),
        ("core.hooksPath", "/dev/null"),
    ] {
        git(&repo, &["config", key, value]);
    }
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
    let before = kagi_git::StateSummary {
        head: local_oid.clone(),
        dirty: "clean".into(),
    };
    let frozen = kagi::remote::resolve_pull_identity(&host, repo.to_str().unwrap()).unwrap();
    let report = kagi::remote::remote_pull(&host, repo.to_str().unwrap(), &frozen, &before);
    assert!(
        report.result.is_ok(),
        "explicit merge pull must succeed despite rebase/ff-only config: {:?}",
        report.result
    );
    assert!(matches!(
        report.recording.entry().outcome,
        kagi_git::oplog::OpOutcome::Success { .. }
    ));
    let commit = git(&repo, &["rev-list", "--parents", "-n", "1", "HEAD"]);
    let parents = commit.split_whitespace().collect::<Vec<_>>();
    assert_eq!(
        parents.len(),
        3,
        "pull must create a merge commit: {commit}"
    );
    assert_eq!(parents[1], local_oid, "local commit must not be rebased");
    assert_eq!(parents[2], upstream_oid, "upstream commit must be merged");
    assert_eq!(git(&repo, &["show", "HEAD:local"]), "local");
    assert_eq!(git(&repo, &["show", "HEAD:upstream"]), "upstream");
}

/// Dirty host changes must never be hidden by Git's implicit autostash, which
/// can exit successfully despite a failed reapplication and a conflicted tree.
#[test]
fn remote_pull_disables_host_autostash_instead_of_recording_conflicted_success() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let origin = root.path().join("origin.git");
    let seed = root.path().join("seed");
    let repo = root.path().join("clone");
    let bin = root.path().join("bin");
    let logs = root.path().join("logs");
    for dir in [&origin, &seed, &bin] {
        std::fs::create_dir_all(dir).unwrap();
    }
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    git(&seed, &["init", "-q", "-b", "main"]);
    std::fs::write(seed.join("file"), "base\n").unwrap();
    git(&seed, &["add", "file"]);
    git(&seed, &["commit", "-qm", "base"]);
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
    std::fs::write(seed.join("file"), "upstream change\n").unwrap();
    git(&seed, &["commit", "-qam", "upstream"]);
    git(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
    std::fs::write(repo.join("file"), "local uncommitted change\n").unwrap();
    for (key, value) in [
        ("merge.autoStash", "true"),
        ("rebase.autoStash", "true"),
        ("submodule.recurse", "true"),
    ] {
        git(&repo, &["config", key, value]);
    }
    let head = git(&repo, &["rev-parse", "HEAD"]);
    let status = git(&repo, &["status", "--porcelain"]);
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
    let before = kagi_git::StateSummary {
        head: head.clone(),
        dirty: "dirty".into(),
    };
    let frozen = kagi::remote::resolve_pull_identity(&host, repo.to_str().unwrap()).unwrap();
    let report = kagi::remote::remote_pull(&host, repo.to_str().unwrap(), &frozen, &before);
    assert!(
        report.result.is_err(),
        "dirty overlapping pull must fail without implicit stash: {:?}",
        report.result
    );
    assert!(
        !matches!(
            report.recording.entry().outcome,
            kagi_git::oplog::OpOutcome::Success { .. }
        ),
        "a failed pull may be conservatively Unknown, but must never be recorded as Success"
    );
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&repo, &["status", "--porcelain"]), status);
    assert_eq!(
        std::fs::read_to_string(repo.join("file")).unwrap(),
        "local uncommitted change\n"
    );
    assert!(git(&repo, &["stash", "list"]).is_empty());
}

#[test]
fn remote_pull_does_not_recurse_into_host_submodule_worktrees() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let sub_origin = root.path().join("sub.git");
    let sub_seed = root.path().join("sub-seed");
    let origin = root.path().join("origin.git");
    let seed = root.path().join("seed");
    let repo = root.path().join("clone");
    let bin = root.path().join("bin");
    let logs = root.path().join("logs");
    for dir in [&sub_origin, &sub_seed, &origin, &seed, &bin] {
        std::fs::create_dir_all(dir).unwrap();
    }
    git(&sub_origin, &["init", "-q", "--bare", "-b", "main"]);
    git(&sub_seed, &["init", "-q", "-b", "main"]);
    std::fs::write(sub_seed.join("module-file"), "before\n").unwrap();
    git(&sub_seed, &["add", "module-file"]);
    git(&sub_seed, &["commit", "-qm", "submodule before"]);
    git(
        &sub_seed,
        &["push", "-q", sub_origin.to_str().unwrap(), "main"],
    );
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    git(&seed, &["init", "-q", "-b", "main"]);
    git(
        &seed,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            sub_origin.to_str().unwrap(),
            "sub",
        ],
    );
    git(&seed, &["commit", "-qam", "submodule before"]);
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
    git(
        &repo,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "update",
            "--init",
            "-q",
        ],
    );
    let sub_before = git(&repo.join("sub"), &["rev-parse", "HEAD"]);
    std::fs::write(sub_seed.join("module-file"), "after\n").unwrap();
    git(&sub_seed, &["commit", "-qam", "submodule after"]);
    let sub_after = git(&sub_seed, &["rev-parse", "HEAD"]);
    git(
        &sub_seed,
        &["push", "-q", sub_origin.to_str().unwrap(), "main"],
    );
    git(&seed.join("sub"), &["fetch", "-q", "origin", "main"]);
    git(&seed.join("sub"), &["checkout", "-q", &sub_after]);
    git(&seed, &["add", "sub"]);
    git(&seed, &["commit", "-qm", "submodule after"]);
    git(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
    git(&repo, &["config", "submodule.recurse", "true"]);
    git(&repo, &["config", "protocol.file.allow", "always"]);
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
    let before = kagi_git::StateSummary {
        head: git(&repo, &["rev-parse", "HEAD"]),
        dirty: "clean".into(),
    };
    let frozen = kagi::remote::resolve_pull_identity(&host, repo.to_str().unwrap()).unwrap();
    let report = kagi::remote::remote_pull(&host, repo.to_str().unwrap(), &frozen, &before);
    assert!(
        report.result.is_ok(),
        "superproject pull must succeed: {:?}",
        report.result
    );
    assert_eq!(git(&repo, &["rev-parse", "HEAD:sub"]), sub_after);
    assert_eq!(
        git(&repo.join("sub"), &["rev-parse", "HEAD"]),
        sub_before,
        "remote pull must not update the submodule worktree implicitly"
    );
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
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = -G ]; then\n\
             printf 'hostname fixture.invalid\\nuser alice\\nport 22\\nidentityfile none\\nuserknownhostsfile none\\nglobalknownhostsfile none\\nproxyjump none\\n'\n\
             exit 0\n\
             fi\n\
             echo '{stderr_line}' >&2\n\
             exit 255\n"
        ),
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

    let frozen = kagi::remote::PullRepoIdentity {
        repo_id: kagi_domain::remote::RemoteRepoId {
            connection: std::sync::Arc::new(kagi_domain::remote::RemoteConnectionId {
                hostname: "fixture.invalid".into(),
                user: "alice".into(),
                port: 22,
                host_key_alias: None,
                proxy_jump: None,
                proxy_command: None,
                control_master: None,
                control_path: None,
                identity_files: vec![],
                certificate_files: vec![],
                user_known_hosts: vec![],
                global_known_hosts: vec![],
                host_key_algorithms: vec![],
            }),
            common_dir: "/srv/repo/.git".into(),
        },
        physical_toplevel: "/srv/repo".into(),
        head: kagi_domain::remote::RemotePullHead {
            branch: Some("main".into()),
            oid: "a".repeat(40),
            upstream: Some("origin/main".into()),
        },
        config: Some(kagi_domain::remote::RemotePullConfig {
            remote_name: "origin".into(),
            merge_ref: "refs/heads/main".into(),
            remote_url: "ssh://fixture.invalid/repo".into(),
            fetch_refspecs: vec!["+refs/heads/*:refs/remotes/origin/*".into()],
        }),
        fingerprint: kagi_domain::remote::RemotePullFingerprint {
            staged: [0; 32],
            worktree: [0; 32],
        },
        remote_dirty: false,
    };
    let report = kagi::remote::remote_pull(&host, "/srv/repo", &frozen, &before);
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
    let kagi_git::oplog::OpOutcome::Failed { error } = outcome else {
        panic!("a recognized auth refusal must stay Failed, got {outcome:?}");
    };
    assert!(
        error.contains("Permission denied (publickey)"),
        "the durable oplog must explain missing agent credentials: {error}"
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
