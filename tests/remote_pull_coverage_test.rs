//! Real-Git coverage of the approved SSH pull's scratch and SHA-256 paths.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use kagi::remote::{self, PullRepoIdentity};
use kagi_domain::remote::RemoteHost;
use kagi_git::{oplog::OpOutcome, StateSummary};
use tempfile::TempDir;

#[path = "support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output as git;
#[path = "support/isolated.rs"]
mod test_support;

#[derive(Clone, Copy)]
enum HashTool {
    Sha256sum,
    Shasum,
    Openssl,
}

impl HashTool {
    fn name(self) -> &'static str {
        match self {
            Self::Sha256sum => "sha256sum",
            Self::Shasum => "shasum",
            Self::Openssl => "openssl",
        }
    }
}

fn system_binary(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .map(|path| path.canonicalize().expect("system utility path"))
}

fn link_binary(bin: &Path, name: &str) {
    symlink(
        system_binary(name).unwrap_or_else(|| panic!("required test utility: {name}")),
        bin.join(name),
    )
    .unwrap();
}

fn install_hash_tool(bin: &Path, tool: HashTool) {
    let path = bin.join(tool.name());
    if let Some(system) = system_binary(tool.name()) {
        symlink(system, path).unwrap();
        return;
    }
    // macOS has shasum but not sha256sum. Minimal Linux images may lack
    // shasum; both adapters still hash the *real file* with a system utility.
    let script = match tool {
        HashTool::Sha256sum => {
            let shasum = system_binary("shasum").expect("shasum or sha256sum required");
            format!(
                "#!/bin/sh\nexec {} -a 256 \"$@\"\n",
                kagi_domain::remote::shell_quote(shasum.to_str().unwrap())
            )
        }
        HashTool::Shasum => {
            let openssl = system_binary("openssl").expect("openssl required for shasum adapter");
            format!(
                "#!/bin/sh\n[ \"$1\" = -a ] && [ \"$2\" = 256 ] && [ \"$#\" -eq 3 ] || exit 91\nsum=$({} dgst -sha256 \"$3\") || exit 1\nprintf '%s  %s\\n' \"${{sum##* }}\" \"$3\"\n",
                kagi_domain::remote::shell_quote(openssl.to_str().unwrap())
            )
        }
        HashTool::Openssl => panic!("openssl is required for the real CLI branch"),
    };
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

struct SavedEnv {
    path: Option<OsString>,
    tmpdir: Option<OsString>,
    log: Option<OsString>,
}

impl Drop for SavedEnv {
    fn drop(&mut self) {
        for (name, value) in [
            ("PATH", &self.path),
            ("TMPDIR", &self.tmpdir),
            ("KAGI_LOG_DIR", &self.log),
        ] {
            if let Some(value) = value {
                std::env::set_var(name, value);
            } else {
                std::env::remove_var(name);
            }
        }
    }
}

struct PullFixture {
    _env: SavedEnv,
    _root: TempDir,
    repo: PathBuf,
    scratch: PathBuf,
    expected_head: String,
    host: RemoteHost,
}

impl PullFixture {
    fn new(tool: HashTool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin.git");
        let seed = root.path().join("seed");
        let repo = root.path().join("clone");
        let bin = root.path().join("bin");
        let scratch = root.path().join("scratch");
        for dir in [&origin, &seed, &bin, &scratch] {
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
        std::fs::write(seed.join("file"), "incoming\n").unwrap();
        git(&seed, &["commit", "-qam", "incoming"]);
        git(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
        let expected_head = git(&seed, &["rev-parse", "HEAD"]);
        assert_ne!(git(&repo, &["rev-parse", "HEAD"]), expected_head);

        for utility in ["sh", "git", "mktemp", "cmp", "rm"] {
            link_binary(&bin, utility);
        }
        install_hash_tool(&bin, tool);
        let ssh = bin.join("ssh");
        std::fs::write(
            &ssh,
            "#!/bin/sh\nif [ \"$1\" = -G ]; then\n  printf 'hostname fixture.invalid\\nuser alice\\nport 22\\nidentityfile none\\nuserknownhostsfile none\\nglobalknownhostsfile none\\nproxyjump none\\n'\n  exit 0\nfi\nfor argument do command=$argument; done\nexec /bin/sh -c \"$command\"\n",
        )
        .unwrap();
        std::fs::set_permissions(ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
        let saved = SavedEnv {
            path: std::env::var_os("PATH"),
            tmpdir: std::env::var_os("TMPDIR"),
            log: std::env::var_os("KAGI_LOG_DIR"),
        };
        std::env::set_var("PATH", &bin);
        std::env::set_var("TMPDIR", &scratch);
        std::env::set_var("KAGI_LOG_DIR", root.path().join("logs"));
        Self {
            _env: saved,
            _root: root,
            repo,
            scratch,
            expected_head,
            host: RemoteHost::parse("fixture.invalid").unwrap(),
        }
    }

    fn plan(&self) -> PullRepoIdentity {
        remote::resolve_pull_identity(&self.host, self.repo.to_str().unwrap()).unwrap()
    }

    fn pull(&self, frozen: &PullRepoIdentity) -> remote::RemotePullReport {
        remote::remote_pull(
            &self.host,
            self.repo.to_str().unwrap(),
            frozen,
            &StateSummary {
                head: frozen.head.oid.clone(),
                dirty: "clean".into(),
            },
        )
    }

    fn assert_no_scratch(&self) {
        let leftovers: Vec<_> = std::fs::read_dir(&self.scratch)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(
            leftovers.is_empty(),
            "preflight scratch remains: {leftovers:?}"
        );
    }
}

#[test]
fn approved_pull_cleans_outside_scratch_on_success_and_refusal() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = PullFixture::new(HashTool::Openssl);
    fixture.assert_no_scratch();
    let frozen = fixture.plan();
    fixture.pull(&frozen).result.expect("real Git fast-forward");
    assert_eq!(
        git(&fixture.repo, &["rev-parse", "HEAD"]),
        fixture.expected_head
    );
    fixture.assert_no_scratch();

    let frozen = fixture.plan();
    std::fs::write(fixture.repo.join("untracked"), "late change\n").unwrap();
    let refusal = fixture.pull(&frozen);
    assert!(matches!(refusal.recording.entry().outcome,
        OpOutcome::Refused { ref blockers } if blockers == &["worktree status changed"]));
    assert_eq!(
        git(&fixture.repo, &["rev-parse", "HEAD"]),
        fixture.expected_head
    );
    fixture.assert_no_scratch();
}

fn assert_selected_hash_branch(tool: HashTool) {
    let fixture = PullFixture::new(tool);
    let frozen = fixture.plan();
    fixture
        .pull(&frozen)
        .result
        .expect("selected SHA-256 branch must accept unchanged index/status");
    assert_eq!(
        git(&fixture.repo, &["rev-parse", "HEAD"]),
        fixture.expected_head
    );
    fixture.assert_no_scratch();

    let frozen = fixture.plan();
    std::fs::write(fixture.repo.join("untracked"), "late change\n").unwrap();
    let refusal = fixture.pull(&frozen);
    assert!(matches!(refusal.recording.entry().outcome,
        OpOutcome::Refused { ref blockers } if blockers == &["worktree status changed"]));
    assert_eq!(
        git(&fixture.repo, &["rev-parse", "HEAD"]),
        fixture.expected_head
    );
    fixture.assert_no_scratch();
}

#[test]
fn sha256sum_hashes_approved_state_and_rejects_late_changes() {
    if !test_support::run_isolated() {
        return;
    }
    assert_selected_hash_branch(HashTool::Sha256sum);
}

#[test]
fn shasum_hashes_approved_state_and_rejects_late_changes() {
    if !test_support::run_isolated() {
        return;
    }
    assert_selected_hash_branch(HashTool::Shasum);
}

#[test]
fn openssl_hashes_approved_state_and_rejects_late_changes() {
    if !test_support::run_isolated() {
        return;
    }
    assert_selected_hash_branch(HashTool::Openssl);
}
