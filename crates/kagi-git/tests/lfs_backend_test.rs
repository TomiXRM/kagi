//! Backend-only LFS evidence. Run with an official git-lfs on PATH.
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod isolated;

use git_fixture::{git, git_output, init_repo, write_file};
use kagi_domain::plan_note::PlanDisposition;
use kagi_git::{Backend, CommitId, Operation};
use std::{path::Path, process::Command};
use tempfile::TempDir;

fn lfs_available() -> bool {
    let available = Command::new("git-lfs")
        .arg("version")
        .output()
        .is_ok_and(|out| out.status.success());
    if std::env::var_os("CI").is_some() {
        assert!(
            available,
            "CI must install git-lfs for backend integration tests"
        );
    }
    available
}

#[test]
fn lfs_diff_reads_use_historical_attributes_and_never_build_pointer_hunks() {
    if !isolated::run_isolated() || !lfs_available() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let repo = dir.path();
    init_repo(repo, "main");
    let path = Path::new("mechanical/part.step");
    std::fs::create_dir(repo.join("mechanical")).unwrap();
    write_file(
        repo,
        "mechanical/.gitattributes",
        "*.step filter=lfs diff=lfs merge=lfs -text\n",
    );
    git(repo, &["add", "mechanical/.gitattributes"]);
    git(repo, &["commit", "-qm", "LFS attributes"]);
    write_file(repo, "mechanical/part.step", "ISO-10303-21;\nold CAD\n");
    let backend = Backend::open(repo).unwrap();
    backend.stage_file(path).unwrap();
    let staged = backend.staged_file_diff(path).unwrap();
    assert!(staged.is_lfs && staged.is_binary && staged.hunks.is_empty());
    git(repo, &["commit", "-qm", "LFS CAD"]);
    let lfs_commit = CommitId(git_output(repo, &["rev-parse", "HEAD"]));

    write_file(repo, "mechanical/part.step", "ISO-10303-21;\nnew CAD\n");
    let unstaged = backend.unstaged_file_diff(path).unwrap();
    assert!(unstaged.is_lfs && unstaged.is_binary && unstaged.hunks.is_empty());
    write_file(repo, "mechanical/.gitattributes", "# LFS removed\n");
    git(
        repo,
        &["add", "mechanical/.gitattributes", "mechanical/part.step"],
    );
    git(repo, &["commit", "-qm", "plain CAD"]);
    let plain_commit = CommitId(git_output(repo, &["rev-parse", "HEAD"]));

    let historical = backend.commit_file_diff(&lfs_commit, path).unwrap();
    assert!(historical.is_lfs && historical.is_binary && historical.hunks.is_empty());
    let transition = backend.commit_file_diff(&plain_commit, path).unwrap();
    assert!(transition.is_lfs && transition.is_binary && transition.hunks.is_empty());
    let compared = backend
        .compare_file_diff(&lfs_commit, &plain_commit, path)
        .unwrap();
    assert!(compared.is_lfs && compared.is_binary && compared.hunks.is_empty());

    write_file(repo, "mechanical/part.step", "ISO-10303-21;\nplain edit\n");
    git(repo, &["add", "mechanical/part.step"]);
    git(repo, &["commit", "-qm", "plain edit"]);
    let current = CommitId(git_output(repo, &["rev-parse", "HEAD"]));
    let ordinary = backend.commit_file_diff(&current, path).unwrap();
    assert!(!ordinary.is_lfs && !ordinary.is_binary && !ordinary.hunks.is_empty());
}

#[test]
fn nested_lfs_stage_commit_and_push_use_pointer_and_transfer_content() {
    if !isolated::run_isolated() {
        return;
    }
    if !lfs_available() {
        eprintln!("git-lfs unavailable; skipping LFS integration");
        return;
    }

    let dir = TempDir::new().unwrap();
    let local = dir.path().join("local");
    let remote = dir.path().join("remote.git");
    std::fs::create_dir(&local).unwrap();
    init_repo(&local, "main");
    git(
        dir.path(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    std::fs::create_dir(local.join("mechanical")).unwrap();
    write_file(
        &local,
        "mechanical/.gitattributes",
        "*.step filter=lfs diff=lfs merge=lfs -text\n",
    );
    write_file(&local, "plain.txt", "plain base\n");
    git(&local, &["add", "mechanical/.gitattributes", "plain.txt"]);
    git(&local, &["commit", "-qm", "base"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let hook = local.join(".git/hooks/pre-push");
        std::fs::write(
            &hook,
            "#!/bin/sh\nprintf invoked > lfs-hook-invoked\nexit 89\n",
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let path = Path::new("mechanical/part.step");
    let content = b"generated CAD fixture bytes\0with binary content\n";
    std::fs::write(local.join(path), content).unwrap();
    let mut backend = Backend::open(&local).unwrap();
    assert!(backend.is_lfs_path(path).unwrap());
    assert!(!backend.is_lfs_path(Path::new("plain.txt")).unwrap());
    let empty = Path::new("mechanical/empty.step");
    std::fs::write(local.join(empty), b"").unwrap();
    backend.stage_file(empty).unwrap();
    assert_eq!(git_output(&local, &["show", ":mechanical/empty.step"]), "");
    backend.stage_file(path).unwrap();
    let pointer = git_output(&local, &["show", ":mechanical/part.step"]);
    assert!(pointer.starts_with("version https://git-lfs.github.com/spec/v1\noid sha256:"));
    assert!(pointer.contains(&format!("size {}", content.len())));
    assert_ne!(pointer.as_bytes(), content);
    assert_eq!(std::fs::read(local.join(path)).unwrap(), content);
    let index = std::fs::read(local.join(".git/index")).unwrap();
    assert!(backend.working_tree_status().unwrap().unstaged.is_empty());

    let later = filetime::FileTime::from_unix_time(1_800_000_000, 0);
    filetime::set_file_mtime(local.join(path), later).unwrap();
    assert!(backend.working_tree_status().unwrap().unstaged.is_empty());
    assert_eq!(std::fs::read(local.join(".git/index")).unwrap(), index);

    let commit = Operation::Commit {
        message: "CAD asset".into(),
    };
    let plan = backend.plan(&commit).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    backend.run(&commit, &plan).unwrap();
    let push = Operation::Push;
    let plan = backend.plan(&push).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    backend.run(&push, &plan).unwrap();
    assert!(!local.join("lfs-hook-invoked").exists());
    assert_eq!(
        git_output(&remote, &["show", "main:mechanical/part.step"]),
        pointer
    );
    let oid = pointer
        .lines()
        .find_map(|line| line.strip_prefix("oid sha256:"))
        .unwrap();
    let object = remote
        .join("lfs/objects")
        .join(&oid[..2])
        .join(&oid[2..4])
        .join(oid);
    assert_eq!(std::fs::read(object).unwrap(), content);

    // A skip-smudge style clone can lack older local LFS objects even though
    // the remote already has them. New content must still be publishable.
    let local_object = local
        .join(".git/lfs/objects")
        .join(&oid[..2])
        .join(&oid[2..4])
        .join(oid);
    std::fs::remove_file(local_object).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        git(&local, &["config", "core.filemode", "false"]);
        std::fs::set_permissions(local.join(path), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(local.join(path), b"new CAD content\0\n").unwrap();
    assert!(backend
        .working_tree_status()
        .unwrap()
        .unstaged
        .iter()
        .any(|s| s.path == path));
    write_file(&local, "plain.txt", "plain changed\n");
    assert_eq!(
        backend
            .stage_files(&["plain.txt".into(), path.into()])
            .unwrap(),
        2
    );
    assert!(backend.working_tree_status().unwrap().unstaged.is_empty());
    let second = git_output(&local, &["show", ":mechanical/part.step"]);
    assert_ne!(pointer, second);
    #[cfg(unix)]
    assert!(
        git_output(&local, &["ls-files", "--stage", "mechanical/part.step"]).starts_with("100644 ")
    );
    assert_eq!(git_output(&local, &["show", ":plain.txt"]), "plain changed");

    let commit = Operation::Commit {
        message: "next CAD asset".into(),
    };
    let plan = backend.plan(&commit).unwrap();
    backend.run(&commit, &plan).unwrap();
    let push = Operation::Push;
    let remote_before = git_output(&remote, &["rev-parse", "refs/heads/main"]);
    git(&local, &["config", "lfs.allowincompletepush", "true"]);
    let unsafe_plan = backend.plan(&push).unwrap();
    assert!(backend
        .run(&push, &unsafe_plan)
        .unwrap_err()
        .to_string()
        .contains("lfs.allowincompletepush"));
    assert_eq!(
        git_output(&remote, &["rev-parse", "refs/heads/main"]),
        remote_before
    );
    git(&local, &["config", "--unset", "lfs.allowincompletepush"]);
    git(&local, &["config", "lfs.pushurl", "file:///unapproved"]);
    let unsafe_plan = backend.plan(&push).unwrap();
    assert!(backend
        .run(&push, &unsafe_plan)
        .unwrap_err()
        .to_string()
        .contains("lfs.pushurl"));
    assert_eq!(
        git_output(&remote, &["rev-parse", "refs/heads/main"]),
        remote_before
    );
    git(&local, &["config", "--unset", "lfs.pushurl"]);
    git(
        &local,
        &[
            "config",
            "--add",
            "remote.origin.pushurl",
            remote.to_str().unwrap(),
        ],
    );
    git(
        &local,
        &[
            "config",
            "--add",
            "remote.origin.pushurl",
            "file:///second-destination",
        ],
    );
    let unsafe_plan = backend.plan(&push).unwrap();
    assert!(backend
        .run(&push, &unsafe_plan)
        .unwrap_err()
        .to_string()
        .contains("multiple Git push URLs"));
    assert_eq!(
        git_output(&remote, &["rev-parse", "refs/heads/main"]),
        remote_before
    );
    git(&local, &["config", "--unset-all", "remote.origin.pushurl"]);
    git(
        &local,
        &[
            "config",
            "--add",
            "remote.origin.url",
            "file:///second-destination",
        ],
    );
    let unsafe_plan = backend.plan(&push).unwrap();
    assert!(backend
        .run(&push, &unsafe_plan)
        .unwrap_err()
        .to_string()
        .contains("multiple Git push URLs"));
    assert_eq!(
        git_output(&remote, &["rev-parse", "refs/heads/main"]),
        remote_before
    );
    git(&local, &["config", "--unset-all", "remote.origin.url"]);
    git(
        &local,
        &["config", "remote.origin.url", remote.to_str().unwrap()],
    );
    let plan = backend.plan(&push).unwrap();
    backend.run(&push, &plan).unwrap();

    git(&local, &["branch", "side"]);
    let checkout = Operation::Checkout {
        branch: "side".into(),
    };
    let blocked = backend.plan(&checkout).unwrap();
    assert_eq!(blocked.disposition, PlanDisposition::Blocked);
    assert!(blocked
        .blockers
        .iter()
        .any(|note| note.message_en().contains("external filter (lfs)")));
    assert!(backend.run(&checkout, &blocked).is_err());
    assert_eq!(git_output(&local, &["branch", "--show-current"]), "main");
    write_file(
        &local,
        "mechanical/.gitattributes",
        "*.step -filter -diff -merge\n",
    );
    let stash = Operation::StashPush {
        message: None,
        include_untracked: false,
    };
    let blocked = backend.plan(&stash).unwrap();
    assert!(blocked
        .blockers
        .iter()
        .any(|note| note.message_en().contains("external filter (lfs)")));
}

#[cfg(unix)]
fn failing_lfs_on_path() -> TempDir {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let script = dir.path().join("git-lfs");
    std::fs::write(&script, "#!/bin/sh\nexit 77\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let old = std::env::var_os("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{}", dir.path().display(), old.to_string_lossy()),
    );
    dir
}

#[cfg(unix)]
#[test]
fn missing_lfs_executable_leaves_index_unchanged() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let local = dir.path().join("local");
    std::fs::create_dir(&local).unwrap();
    init_repo(&local, "main");
    write_file(
        &local,
        ".gitattributes",
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    );
    git(&local, &["add", ".gitattributes"]);
    git(&local, &["commit", "-qm", "attributes"]);
    write_file(&local, "asset.bin", "CAD content\0\n");
    let before = std::fs::read(local.join(".git/index")).unwrap();

    let tools = TempDir::new().unwrap();
    let git_path = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("git"))
        .find(|path| path.is_file())
        .unwrap();
    std::os::unix::fs::symlink(git_path, tools.path().join("git")).unwrap();
    std::env::set_var("PATH", tools.path());

    let backend = Backend::open(&local).unwrap();
    assert!(backend.stage_file(Path::new("asset.bin")).is_err());
    assert_eq!(std::fs::read(local.join(".git/index")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn mixed_batch_failure_does_not_publish_plain_index_changes() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let local = dir.path().join("local");
    std::fs::create_dir(&local).unwrap();
    init_repo(&local, "main");
    write_file(
        &local,
        ".gitattributes",
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    );
    write_file(&local, "plain.txt", "before\n");
    git(&local, &["add", ".gitattributes", "plain.txt"]);
    git(&local, &["commit", "-qm", "base"]);
    write_file(&local, "plain.txt", "after\n");
    write_file(&local, "asset.bin", "binary content\0\n");
    let before = std::fs::read(local.join(".git/index")).unwrap();
    let _fake = failing_lfs_on_path();
    let backend = Backend::open(&local).unwrap();
    assert!(backend
        .stage_files(&["plain.txt".into(), "asset.bin".into()])
        .is_err());
    assert_eq!(std::fs::read(local.join(".git/index")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn lockable_attribute_added_during_clean_aborts_stage() {
    if !isolated::run_isolated() {
        return;
    }
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let local = dir.path().join("local");
    std::fs::create_dir(&local).unwrap();
    init_repo(&local, "main");
    write_file(
        &local,
        ".gitattributes",
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    );
    git(&local, &["add", ".gitattributes"]);
    git(&local, &["commit", "-qm", "attributes"]);
    let bytes = b"CAD bytes\0\n";
    std::fs::write(local.join("asset.bin"), bytes).unwrap();
    let before = std::fs::read(local.join(".git/index")).unwrap();

    let tools = TempDir::new().unwrap();
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\\noid sha256:{}\\nsize {}\\n",
        hex::encode(Sha256::digest(bytes)),
        bytes.len()
    );
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' '*.bin filter=lfs diff=lfs merge=lfs lockable -text' > .gitattributes\nprintf '{pointer}'\n"
    );
    let tool = tools.path().join("git-lfs");
    std::fs::write(&tool, script).unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    let old = std::env::var_os("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{}", tools.path().display(), old.to_string_lossy()),
    );

    let backend = Backend::open(&local).unwrap();
    assert!(backend.stage_file(Path::new("asset.bin")).is_err());
    assert_eq!(std::fs::read(local.join(".git/index")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn failed_lfs_upload_does_not_publish_git_ref() {
    if !isolated::run_isolated() {
        return;
    }
    if !lfs_available() {
        eprintln!("git-lfs unavailable; skipping LFS integration");
        return;
    }
    let dir = TempDir::new().unwrap();
    let local = dir.path().join("local");
    let remote = dir.path().join("remote.git");
    std::fs::create_dir(&local).unwrap();
    init_repo(&local, "main");
    git(
        dir.path(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    std::fs::create_dir(local.join("mechanical")).unwrap();
    write_file(
        &local,
        "mechanical/.gitattributes",
        "*.step filter=lfs diff=lfs merge=lfs -text\n",
    );
    write_file(&local, "mechanical/part.step", "CAD version 1\n");
    let mut backend = Backend::open(&local).unwrap();
    backend
        .stage_files(&[
            "mechanical/.gitattributes".into(),
            "mechanical/part.step".into(),
        ])
        .unwrap();
    let commit = Operation::Commit {
        message: "CAD asset".into(),
    };
    let plan = backend.plan(&commit).unwrap();
    backend.run(&commit, &plan).unwrap();
    let _fake = failing_lfs_on_path();
    let push = Operation::Push;
    let plan = backend.plan(&push).unwrap();
    assert!(backend.run(&push, &plan).is_err());
    assert!(git_output(&remote, &["for-each-ref", "--format=%(refname)"]).is_empty());
}

#[cfg(unix)]
#[test]
fn pointer_looking_plain_text_does_not_require_lfs() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let local = dir.path().join("local");
    let remote = dir.path().join("remote.git");
    std::fs::create_dir(&local).unwrap();
    init_repo(&local, "main");
    git(
        dir.path(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    write_file(&local, "ordinary.txt", "version https://git-lfs.github.com/spec/v1\noid sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nsize 42\n");
    git(&local, &["add", "ordinary.txt"]);
    git(&local, &["commit", "-qm", "ordinary text"]);
    let _fake = failing_lfs_on_path();
    let mut backend = Backend::open(&local).unwrap();
    let push = Operation::Push;
    let plan = backend.plan(&push).unwrap();
    backend.run(&push, &plan).unwrap();
    assert_eq!(
        git_output(&remote, &["show", "main:ordinary.txt"]),
        git_output(&local, &["show", "HEAD:ordinary.txt"])
    );
}

#[cfg(unix)]
#[test]
fn attributes_added_after_pointer_still_require_lfs_upload() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let local = dir.path().join("local");
    let remote = dir.path().join("remote.git");
    std::fs::create_dir(&local).unwrap();
    init_repo(&local, "main");
    git(
        dir.path(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    write_file(&local, "asset.bin", "version https://git-lfs.github.com/spec/v1\noid sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nsize 42\n");
    git(&local, &["add", "asset.bin"]);
    git(&local, &["commit", "-qm", "pointer before attributes"]);
    write_file(
        &local,
        ".gitattributes",
        "asset.bin filter=lfs diff=lfs merge=lfs -text\n",
    );
    git(&local, &["add", ".gitattributes"]);
    git(&local, &["commit", "-qm", "track existing pointer"]);

    let _fake = failing_lfs_on_path();
    let mut backend = Backend::open(&local).unwrap();
    let push = Operation::Push;
    let plan = backend.plan(&push).unwrap();
    assert!(backend.run(&push, &plan).is_err());
    assert!(git_output(&remote, &["for-each-ref", "--format=%(refname)"]).is_empty());
}

#[test]
fn lockable_lfs_paths_refuse_stage_and_push_without_lock_verification() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let local = dir.path().join("local");
    let remote = dir.path().join("remote.git");
    std::fs::create_dir(&local).unwrap();
    init_repo(&local, "main");
    git(
        dir.path(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    write_file(
        &local,
        ".gitattributes",
        "a.bin filter=lfs diff=lfs merge=lfs -text\nasset.bin filter=lfs diff=lfs merge=lfs -text\n",
    );
    write_file(&local, "a.bin", "version https://git-lfs.github.com/spec/v1\noid sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\nsize 42\n");
    write_file(&local, "asset.bin", "version https://git-lfs.github.com/spec/v1\noid sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nsize 42\n");
    git(&local, &["add", ".gitattributes", "a.bin", "asset.bin"]);
    git(&local, &["commit", "-qm", "pre-existing pointer"]);
    write_file(
        &local,
        ".gitattributes",
        "a.bin filter=lfs diff=lfs merge=lfs -text\nasset.bin filter=lfs diff=lfs merge=lfs lockable -text\n",
    );
    git(&local, &["add", ".gitattributes"]);
    git(&local, &["commit", "-qm", "mark old pointer lockable"]);
    let mut backend = Backend::open(&local).unwrap();
    let before = std::fs::read(local.join(".git/index")).unwrap();
    write_file(&local, "asset.bin", "new content\n");
    assert!(backend
        .stage_file(Path::new("asset.bin"))
        .unwrap_err()
        .to_string()
        .contains("lockable"));
    assert_eq!(std::fs::read(local.join(".git/index")).unwrap(), before);
    let push = Operation::Push;
    let plan = backend.plan(&push).unwrap();
    assert!(backend
        .run(&push, &plan)
        .unwrap_err()
        .to_string()
        .contains("lockable"));
    assert!(git_output(&remote, &["for-each-ref", "--format=%(refname)"]).is_empty());
}

#[test]
fn incoming_only_lfs_checkout_and_pull_refuse_before_worktree_write() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let local = dir.path().join("local");
    let remote = dir.path().join("remote.git");
    let other = dir.path().join("other");
    std::fs::create_dir(&local).unwrap();
    init_repo(&local, "main");
    write_file(&local, "plain.txt", "base\n");
    git(&local, &["add", "plain.txt"]);
    git(&local, &["commit", "-qm", "base"]);
    let base = git_output(&local, &["rev-parse", "HEAD"]);

    git(&local, &["checkout", "-qb", "incoming"]);
    std::fs::create_dir(local.join("mechanical")).unwrap();
    write_file(
        &local,
        "mechanical/.gitattributes",
        "*.step filter=lfs diff=lfs merge=lfs -text\n",
    );
    write_file(&local, "mechanical/part.step", "version https://git-lfs.github.com/spec/v1\noid sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nsize 42\n");
    git(&local, &["add", "mechanical"]);
    git(&local, &["commit", "-qm", "incoming pointer"]);
    git(&local, &["checkout", "-q", "main"]);
    let mut backend = Backend::open(&local).unwrap();
    let checkout = Operation::Checkout {
        branch: "incoming".into(),
    };
    let plan = backend.plan(&checkout).unwrap();
    assert_eq!(plan.disposition, PlanDisposition::Blocked);
    assert!(plan
        .blockers
        .iter()
        .any(|n| n.message_en().contains("external filter (lfs)")));
    assert!(backend.run(&checkout, &plan).is_err());
    assert_eq!(git_output(&local, &["rev-parse", "HEAD"]), base);
    assert!(!local.join("mechanical/part.step").exists());

    git(
        dir.path(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&local, &["push", "-q", "-u", "origin", "main"]);
    git(
        dir.path(),
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.name", "Other"]);
    git(&other, &["config", "user.email", "other@example.com"]);
    std::fs::create_dir(other.join("mechanical")).unwrap();
    write_file(
        &other,
        "mechanical/.gitattributes",
        "*.step filter=lfs diff=lfs merge=lfs -text\n",
    );
    write_file(&other, "mechanical/part.step", "version https://git-lfs.github.com/spec/v1\noid sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\nsize 10\n");
    git(&other, &["add", "mechanical"]);
    git(&other, &["commit", "-qm", "new remote pointer"]);
    git(&other, &["push", "-q", "origin", "main"]);
    let pull = Operation::Pull;
    let plan = backend.plan(&pull).unwrap();
    assert!(backend.run(&pull, &plan).is_err());
    assert_eq!(git_output(&local, &["rev-parse", "HEAD"]), base);
    assert!(!local.join("mechanical/part.step").exists());
}
