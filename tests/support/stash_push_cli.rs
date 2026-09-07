//! #622: stored bytes and subprocess safety at the existing application boundary.
use super::*;

#[test]
fn push_preserves_index_worktree_and_untracked_trees() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = Fixture::new();
    std::fs::write(f.repo.join(".gitignore"), "ignored\n").unwrap();
    git(&f.repo, &["add", ".gitignore"]);
    git(&f.repo, &["commit", "-qm", "ignore fixture"]);
    std::fs::write(f.repo.join("file"), "staged\n").unwrap();
    git(&f.repo, &["add", "file"]);
    std::fs::write(f.repo.join("file"), "unstaged\n").unwrap();
    std::fs::write(f.repo.join("new [file]"), b"new\0bytes\n").unwrap();
    std::fs::write(f.repo.join("ignored"), "leave alone\n").unwrap();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let c = f
        .job(
            &mut s,
            owner,
            StashAction::Push {
                message: Some("--literal message; $(not-a-command) 日本語".into()),
                include_untracked: true,
            },
        )
        .run();
    assert!(
        matches!(outcome(&c), OpOutcome::Success { .. }),
        "{:?}",
        outcome(&c)
    );
    assert_eq!(git(&f.repo, &["show", "refs/stash^2:file"]), "staged\n");
    assert_eq!(git(&f.repo, &["show", "refs/stash:file"]), "unstaged\n");
    assert_eq!(
        git(&f.repo, &["show", "refs/stash^3:new [file]"]).as_bytes(),
        b"new\0bytes\n"
    );
    assert_eq!(std::fs::read(f.repo.join("file")).unwrap(), b"base\n");
    assert_eq!(
        std::fs::read(f.repo.join("ignored")).unwrap(),
        b"leave alone\n"
    );
    assert!(git(&f.repo, &["status", "--porcelain"]).is_empty());
    assert!(git(&f.repo, &["show", "-s", "--format=%s", "refs/stash"])
        .contains("--literal message; $(not-a-command) 日本語"));
    assert_eq!(
        c.report().evidence.oid.as_deref(),
        Some(f.ids()[0].as_str())
    );
    assert_eq!(read_oplog_tail(100).len(), 1);
}

#[cfg(unix)]
#[test]
fn push_does_not_execute_configured_filters_or_hooks() {
    use std::os::unix::fs::PermissionsExt;
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = Fixture::new();
    let marker = f._dir.path().join("executed");
    let script = f._dir.path().join("external");
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(f.repo.join(".gitattributes"), "file filter=Probe\n").unwrap();
    git(
        &f.repo,
        &["config", "filter.Probe.clean", script.to_str().unwrap()],
    );
    git(
        &f.repo,
        &["config", "filter.Probe.smudge", script.to_str().unwrap()],
    );
    git(
        &f.repo,
        &["config", "filter.Probe.process", script.to_str().unwrap()],
    );
    git(&f.repo, &["config", "filter.Probe.required", "true"]);
    let hooks = f._dir.path().join("hooks");
    std::fs::create_dir(&hooks).unwrap();
    std::fs::copy(&script, hooks.join("reference-transaction")).unwrap();
    git(
        &f.repo,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
    std::fs::write(f.repo.join("file"), "saved without external code\n").unwrap();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let c = f
        .job(
            &mut s,
            owner,
            StashAction::Push {
                message: None,
                include_untracked: true,
            },
        )
        .run();
    assert!(
        matches!(outcome(&c), OpOutcome::Success { .. }),
        "{:?}",
        outcome(&c)
    );
    assert!(
        !marker.exists(),
        "stash executed a configured filter or hook"
    );
    assert_eq!(
        git(&f.repo, &["show", "refs/stash:file"]),
        "saved without external code\n"
    );
}

#[cfg(unix)]
#[test]
fn incomplete_push_capture_records_unknown_and_requires_reconcile() {
    use std::os::unix::fs::PermissionsExt;
    if !crate::test_support::run_isolated() {
        return;
    }
    let f = Fixture::new();
    std::fs::write(f.repo.join("file"), "saved before capture failure\n").unwrap();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let job = f.job(
        &mut s,
        owner,
        StashAction::Push {
            message: None,
            include_untracked: true,
        },
    );
    let id = job.id();
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let real_git = String::from_utf8(real_git.stdout).unwrap();
    let bin = f._dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let wrapper = bin.join("git");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n'{}' \"$@\" || exit $?\nsleep 4 &\nexit 0\n",
            real_git.trim().replace('\'', "'\\''")
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let old_path = std::env::var_os("PATH").unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&old_path));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    let c = job.run();
    std::env::set_var("PATH", old_path);
    assert!(
        matches!(outcome(&c), OpOutcome::Unknown { .. }),
        "{:?}",
        outcome(&c)
    );
    assert!(c.report().evidence.unknown);
    assert!(!c.report().evidence.verified);
    assert_eq!(
        git(&f.repo, &["show", "refs/stash:file"]),
        "saved before capture failure\n"
    );
    assert!(matches!(
        read_oplog_tail(100)[0].outcome,
        OpOutcome::Unknown { .. }
    ));
    s.apply(c);
    assert!(matches!(
        s.write_lease(&f.repo, LegacyBusy(false)),
        Err(AdmissionError::NeedsReconcile)
    ));
    let read = prepare_reconcile(&s, id).unwrap().run().unwrap();
    acknowledge(&mut s, read).unwrap();
    s.write_lease(&f.repo, LegacyBusy(false))
        .unwrap()
        .complete();
    assert_eq!(read_oplog_tail(100).len(), 1);
}
