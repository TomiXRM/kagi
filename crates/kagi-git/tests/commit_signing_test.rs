//! #1133: Git's configured signing policy must not produce an unsigned success.
use kagi_git::{
    oplog::{read_oplog_tail_for_repo, OpOutcome},
    Backend, Operation,
};
use std::{fs, process::Command};
use tempfile::TempDir;
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;
fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    let repo = dir.path();
    init_repo(repo, "main");
    git(repo, &["config", "core.hooksPath", "/dev/null"]);
    write_file(repo, "a", "base\n");
    commit_all(repo, "base");
    write_file(repo, "b", "second\n");
    commit_all(repo, "second");
    write_file(repo, "a", "approved\n");
    git(repo, &["add", "a"]);
    git(repo, &["config", "commit.gpgsign", "true"]);
    git(repo, &["config", "gpg.format", "ssh"]);
    git(repo, &["config", "gpg.ssh.program", "ssh-keygen"]);
    dir
}

fn configure_signing_key(repo: &std::path::Path) {
    let key = repo.join("throwaway-key");
    let output = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&key)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    git(repo, &["config", "user.signingkey", key.to_str().unwrap()]);
    let public = fs::read_to_string(key.with_extension("pub")).unwrap();
    let allowed = repo.join("allowed-signers");
    fs::write(&allowed, format!("test@example.com {}", public)).unwrap();
    git(
        repo,
        &[
            "config",
            "gpg.ssh.allowedSignersFile",
            allowed.to_str().unwrap(),
        ],
    );
}

#[test]
fn commit_signing_missing_ssh_key_keeps_head_and_index() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    let missing = repo.join("missing-key");
    git(
        repo,
        &["config", "user.signingkey", missing.to_str().unwrap()],
    );
    let head = git_output(repo, &["rev-parse", "HEAD"]);
    let index = fs::read(repo.join(".git/index")).unwrap();
    let mut backend = Backend::open(repo).unwrap();
    let op = Operation::Commit {
        message: "signed".into(),
    };
    let plan = backend.plan(&op).unwrap();
    let report = backend.run_recorded(&op, &plan);
    assert!(
        report.result.is_err(),
        "missing required key cannot create a commit: {:?}",
        report.result
    );
    assert_eq!(git_output(repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
    let tail = read_oplog_tail_for_repo(repo, 10);
    assert_eq!(tail.len(), 1);
    assert!(
        matches!(&tail[0].outcome, OpOutcome::Failed { error } if error.contains("missing-key"))
    );
}
#[test]
fn commit_signing_ssh_commit_and_amend_have_verified_signatures() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    configure_signing_key(repo);
    let mut backend = Backend::open(repo).unwrap();
    for op in [
        Operation::Commit {
            message: "signed".into(),
        },
        Operation::Amend {
            mode: kagi_git::AmendMode::MessageOnly,
            message: Some("signed amend".into()),
        },
        Operation::Amend {
            mode: kagi_git::AmendMode::Staged,
            message: None,
        },
        Operation::Amend {
            mode: kagi_git::AmendMode::Both,
            message: Some("signed both".into()),
        },
    ] {
        if matches!(&op, Operation::Amend { mode, .. } if mode.includes_staged()) {
            write_file(repo, "a", &format!("approved {op:?}\n"));
            git(repo, &["add", "a"]);
        }
        let tree = git_output(repo, &["write-tree"]);
        let plan = backend.plan(&op).unwrap();
        backend.run(&op, &plan).unwrap();
        assert!(git_output(repo, &["cat-file", "-p", "HEAD"])
            .contains("\ngpgsig -----BEGIN SSH SIGNATURE-----"));
        git(repo, &["verify-commit", "HEAD"]);
        assert_eq!(git_output(repo, &["rev-parse", "HEAD^{tree}"]), tree);
    }
}

#[cfg(unix)]
#[test]
fn commit_signing_unsigned_post_commit_replacement_is_not_success() {
    use std::os::unix::fs::PermissionsExt;
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    configure_signing_key(repo);
    git(repo, &["config", "core.hooksPath", ".git/hooks"]);
    let hook = repo.join(".git/hooks/post-commit");
    fs::write(&hook, "#!/bin/sh\ngit -c core.hooksPath=/dev/null -c commit.gpgsign=false commit --amend --no-edit\n").unwrap();
    fs::set_permissions(hook, fs::Permissions::from_mode(0o755)).unwrap();
    let mut backend = Backend::open(repo).unwrap();
    let op = Operation::Commit {
        message: "signed".into(),
    };
    let plan = backend.plan(&op).unwrap();
    let report = backend.run_recorded(&op, &plan);
    assert!(
        report.result.is_err(),
        "a required signature missing from final HEAD must not be success"
    );
    assert!(
        matches!(&report.recording.entry().outcome, OpOutcome::Unknown { evidence, .. }
        if evidence.contains("signature"))
    );
}

#[test]
fn commit_signing_ssh_resolved_merge_is_signed_and_keeps_both_parents() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    git(repo, &["config", "commit.gpgsign", "false"]);
    commit_all(repo, "approved base");
    git(repo, &["checkout", "-qb", "side"]);
    write_file(repo, "side", "side\n");
    commit_all(repo, "side");
    let side = git_output(repo, &["rev-parse", "HEAD"]);
    git(repo, &["checkout", "-q", "main"]);
    write_file(repo, "main", "main\n");
    commit_all(repo, "main");
    git(repo, &["merge", "--no-commit", "--no-ff", "side"]);
    let head = git_output(repo, &["rev-parse", "HEAD"]);
    let tree = git_output(repo, &["write-tree"]);
    configure_signing_key(repo);
    git(repo, &["config", "commit.gpgsign", "true"]);
    let mut backend = Backend::open(repo).unwrap();
    let op = Operation::MergeCommit {
        message: "signed merge".into(),
    };
    let plan = backend.plan(&op).unwrap();
    backend.run(&op, &plan).unwrap();
    assert!(git_output(repo, &["cat-file", "-p", "HEAD"])
        .contains("\ngpgsig -----BEGIN SSH SIGNATURE-----"));
    git(repo, &["verify-commit", "HEAD"]);
    assert_eq!(git_output(repo, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(
        git_output(repo, &["log", "-1", "--format=%P"]),
        format!("{head} {side}")
    );
    assert!(!repo.join(".git/MERGE_HEAD").exists());
}

#[test]
fn commit_signing_hasconfig_include_uses_gits_effective_policy() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    let included = repo.join(".git/signing-policy");
    fs::write(&included, "[commit]\n\tgpgsign = false\n").unwrap();
    git(
        repo,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    git(
        repo,
        &[
            "config",
            "includeIf.hasconfig:remote.*.url:**.path",
            included.to_str().unwrap(),
        ],
    );
    assert_eq!(
        git_output(repo, &["config", "--type=bool", "--get", "commit.gpgsign"]),
        "false"
    );
    let mut backend = Backend::open(repo).unwrap();
    let op = Operation::Commit {
        message: "effective unsigned policy".into(),
    };
    let plan = backend.plan(&op).unwrap();
    let report = backend.run_recorded(&op, &plan);
    report.result.unwrap();
    assert!(matches!(
        &report.recording.entry().outcome,
        OpOutcome::Success { .. }
    ));
    assert!(!git_output(repo, &["cat-file", "-p", "HEAD"]).contains("\ngpgsig "));
}
