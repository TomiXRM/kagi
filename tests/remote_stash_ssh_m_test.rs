//! Opt-in localhost SSH acceptance test for FAMILY-stash r5 PR 2.
//!
//! Run only with an explicit direct-key localhost alias:
//! `KAGI_M_SSH_HOST=kagi-m-localhost KAGI_LOG_DIR=$(mktemp -d) cargo test \
//!   -p kagi --test remote_stash_ssh_m_test -- --ignored --nocapture`

use kagi::app::{
    self, approve, plan_remote_stash, AdmissionError, LegacyBusy, PlanState, PlanToken, Planned,
    RemoteStashRequest, Sessions, StashPolicy,
};
use kagi::remote::stash::RemoteAttachment;
use kagi_domain::remote::{
    parse_effective_ssh_config, EffectiveSshConfig, RemoteDropOutcome, RemoteHost, RemoteRepoId,
};
use kagi_git::oplog::{entry_to_json, read_oplog_tail};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

#[path = "support/remote_stash.rs"]
mod remote_stash_support;

const REAL_SSH: &str = "/usr/bin/ssh";
const REAL_GIT: &str = "/usr/bin/git";

struct EnvGuard {
    old: Vec<(&'static str, Option<OsString>)>,
}

impl EnvGuard {
    fn set(values: &[(&'static str, OsString)]) -> Self {
        let old = values
            .iter()
            .map(|(name, _)| (*name, std::env::var_os(name)))
            .collect();
        for (name, value) in values {
            std::env::set_var(name, value);
        }
        Self { old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, value) in self.old.iter().rev() {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

fn checked(mut command: Command, context: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{context}: {error}"));
    assert!(
        output.status.success(),
        "{context}: status={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn git(repo: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(REAL_GIT);
    command
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Kagi M")
        .env("GIT_AUTHOR_EMAIL", "kagi-m@example.com")
        .env("GIT_COMMITTER_NAME", "Kagi M")
        .env("GIT_COMMITTER_EMAIL", "kagi-m@example.com");
    checked(command, &format!("git {args:?}"))
}

fn git_text(repo: &Path, args: &[&str]) -> String {
    String::from_utf8(git(repo, args).stdout)
        .expect("git output must be UTF-8")
        .trim()
        .to_owned()
}

fn effective_config(host: &RemoteHost) -> EffectiveSshConfig {
    let mut command = Command::new(REAL_SSH);
    command
        .arg("-G")
        .args(host.connection_opts())
        .arg("--")
        .arg(host.target());
    let output = checked(command, "ssh -G for M alias");
    let text = String::from_utf8(output.stdout).expect("ssh -G output must be UTF-8");
    parse_effective_ssh_config(&text).expect("M alias must satisfy the frozen SSH policy")
}

fn one_line<'a>(value: &'a str, name: &str) -> &'a str {
    assert!(
        !value.is_empty() && !value.chars().any(char::is_whitespace),
        "{name} must be a non-empty SSH config atom: {value:?}"
    );
    value
}

fn write_ssh_config(
    path: &Path,
    alias: &str,
    effective: &EffectiveSshConfig,
    identity: &str,
    hostname: &str,
) {
    use std::os::unix::fs::PermissionsExt;

    let mut bytes = Vec::new();
    writeln!(bytes, "Host {}", one_line(alias, "alias")).unwrap();
    writeln!(bytes, "  HostName {}", one_line(hostname, "hostname")).unwrap();
    writeln!(bytes, "  User {}", one_line(&effective.user, "user")).unwrap();
    writeln!(bytes, "  Port {}", effective.port).unwrap();
    writeln!(bytes, "  IdentityFile {}", one_line(identity, "identity")).unwrap();
    writeln!(bytes, "  IdentitiesOnly yes").unwrap();
    writeln!(bytes, "  IdentityAgent none").unwrap();
    for certificate in &effective.certificate_files {
        writeln!(
            bytes,
            "  CertificateFile {}",
            one_line(certificate, "certificate")
        )
        .unwrap();
    }
    writeln!(bytes, "  StrictHostKeyChecking yes").unwrap();
    writeln!(
        bytes,
        "  UserKnownHostsFile {}",
        effective
            .user_known_hosts_files
            .iter()
            .map(|value| one_line(value, "user known_hosts"))
            .collect::<Vec<_>>()
            .join(" ")
    )
    .unwrap();
    writeln!(
        bytes,
        "  GlobalKnownHostsFile {}",
        if effective.global_known_hosts_files.is_empty() {
            "none".to_owned()
        } else {
            effective
                .global_known_hosts_files
                .iter()
                .map(|value| one_line(value, "global known_hosts"))
                .collect::<Vec<_>>()
                .join(" ")
        }
    )
    .unwrap();
    writeln!(
        bytes,
        "  HostKeyAlgorithms {}",
        effective.host_key_algorithms.join(",")
    )
    .unwrap();
    if let Some(host_key_alias) = &effective.host_key_alias {
        writeln!(
            bytes,
            "  HostKeyAlias {}",
            one_line(host_key_alias, "host key alias")
        )
        .unwrap();
    }
    writeln!(bytes, "  ProxyJump none").unwrap();
    writeln!(bytes, "  ProxyCommand none").unwrap();
    writeln!(bytes, "  ControlMaster no").unwrap();
    writeln!(bytes, "  ControlPath none").unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn install_ssh_wrapper(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(
        path,
        r#"#!/bin/bash
set -eu
for arg in "$@"; do
  if [[ "$arg" == "-G" ]]; then
    exec "$KAGI_M_REAL_SSH" -F "$HOME/.ssh/config" "$@"
  fi
done
last="${!#}"
if [[ "$last" == *"KAGI-"* || "$last" == *"remote-ops"* ]]; then
  runtime=$(printf '%q' "$KAGI_M_REMOTE_RUNTIME")
  transformed="export XDG_RUNTIME_DIR=$runtime; $last"
  if [[ "$last" == *"KAGI-STASH-DROP"* ]]; then
    printf '%s\n' "$$" >"$KAGI_M_SSH_PID_FILE"
    shim=$(printf '%q' "$KAGI_M_REMOTE_GIT_BIN")
    block=$(printf '%q' "$KAGI_M_SHIM_BLOCK")
    entered=$(printf '%q' "$KAGI_M_SHIM_ENTERED")
    release=$(printf '%q' "$KAGI_M_SHIM_RELEASE")
    real_git=$(printf '%q' "$KAGI_M_REAL_GIT")
    transformed="export XDG_RUNTIME_DIR=$runtime; export PATH=$shim:/usr/bin:/bin:/usr/sbin:/sbin; export KAGI_M_SHIM_BLOCK=$block KAGI_M_SHIM_ENTERED=$entered KAGI_M_SHIM_RELEASE=$release KAGI_M_REAL_GIT=$real_git; trap '' HUP TERM; $last"
  fi
  argv=("${@:1:$#-1}")
  exec "$KAGI_M_REAL_SSH" "${argv[@]}" "$transformed"
fi
exec "$KAGI_M_REAL_SSH" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn install_git_shim(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(
        path,
        r#"#!/bin/sh
set -eu
if test "${1:-}" = stash && test "${2:-}" = drop && test -f "$KAGI_M_SHIM_BLOCK"; then
  : >"$KAGI_M_SHIM_ENTERED"
  trap '' HUP TERM
  while test ! -f "$KAGI_M_SHIM_RELEASE"; do /bin/sleep 0.05; done
fi
exec "$KAGI_M_REAL_GIT" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[derive(Debug, PartialEq, Eq)]
struct RepoInvariant {
    head: String,
    index: String,
    worktree: Vec<u8>,
}

fn invariant(repo: &Path) -> RepoInvariant {
    let index_path = git_text(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    RepoInvariant {
        head: git_text(repo, &["rev-parse", "--verify", "HEAD"]),
        index: git_text(repo, &["hash-object", &index_path]),
        worktree: git(
            repo,
            &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        )
        .stdout,
    }
}

fn stash_oids(repo: &Path) -> Vec<String> {
    git_text(repo, &["stash", "list", "--format=%H"])
        .lines()
        .map(str::to_owned)
        .collect()
}

fn wait_for(path: &Path, context: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    assert!(
        path.exists(),
        "timed out waiting for {context}: {}",
        path.display()
    );
}

struct BlockedRemote {
    handle: Option<thread::JoinHandle<app::Completion>>,
    release: PathBuf,
    released: bool,
}

impl BlockedRemote {
    fn start(job: app::Job, release: PathBuf) -> Self {
        Self {
            handle: Some(thread::spawn(move || job.run())),
            release,
            released: false,
        }
    }

    fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .is_none_or(|handle| handle.is_finished())
    }

    fn channel_result(&mut self) -> app::Completion {
        self.handle
            .take()
            .expect("channel result is consumed once")
            .join()
            .expect("remote job thread")
    }

    fn release(&mut self) {
        fs::write(&self.release, b"release\n").unwrap();
        self.released = true;
    }
}

impl Drop for BlockedRemote {
    fn drop(&mut self) {
        if !self.released {
            let _ = fs::write(&self.release, b"release\n");
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn assert_no_kagi_trace(root: &Path) {
    fn visit(path: &Path) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            assert!(
                !name.contains("kagi"),
                "Kagi trace inside repository: {}",
                path.display()
            );
            if entry.file_type().unwrap().is_dir() {
                visit(&path);
            }
        }
    }
    visit(root);
}

fn request(host: &RemoteHost, root: &Path, index: usize) -> RemoteStashRequest {
    RemoteStashRequest {
        owner: RemoteAttachment {
            host: host.clone(),
            root: root.display().to_string(),
            generation: 1,
        },
        index,
    }
}

struct ReadyRemote {
    token: PlanToken,
    repo_id: RemoteRepoId,
    selected_oid: String,
}

fn ready(sessions: &mut Sessions, host: &RemoteHost, root: &Path, index: usize) -> ReadyRemote {
    let policy = StashPolicy::default();
    let completion = plan_remote_stash(sessions, request(host, root, index), policy).run();
    assert!(app::apply_plan(sessions, completion));
    let PlanState::Ready { token, prepared } = sessions.plan_state() else {
        panic!(
            "remote stash plan was not ready: {:?}",
            sessions.plan_state()
        )
    };
    let Planned::RemoteStash { plan, .. } = prepared else {
        panic!("remote plan installed the wrong family")
    };
    ReadyRemote {
        token: token.clone(),
        repo_id: plan.repo_id.clone(),
        selected_oid: plan.selected_oid.clone(),
    }
}

fn prepare_job(sessions: &mut Sessions, prepared: ReadyRemote) -> app::Job {
    let approved = approve(sessions, prepared.token, StashPolicy::default()).unwrap();
    app::prepare(sessions, approved, LegacyBusy(false)).unwrap()
}

fn remove_if_present(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("remove {}: {error}", path.display()),
    }
}

#[test]
#[ignore = "requires KAGI_M_SSH_HOST direct-key localhost alias"]
fn remote_stash_drop_over_real_localhost_ssh() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let Some(alias_spec) = std::env::var_os("KAGI_M_SSH_HOST") else {
        eprintln!("skipping: set KAGI_M_SSH_HOST to the localhost SSH alias");
        return;
    };
    assert!(
        std::env::var_os("KAGI_LOG_DIR").is_some(),
        "set KAGI_LOG_DIR to a fresh directory"
    );
    assert!(Path::new(REAL_SSH).is_file(), "M requires macOS system ssh");
    assert!(Path::new(REAL_GIT).is_file(), "M requires macOS system git");

    let alias_spec = alias_spec.to_string_lossy().into_owned();
    let mut host = RemoteHost::parse(&alias_spec).expect("KAGI_M_SSH_HOST must parse");
    let initial = effective_config(&host);
    assert_eq!(
        initial.hostname, "127.0.0.1",
        "M alias must target localhost"
    );
    let identity = initial
        .identity_files
        .iter()
        .find(|path| Path::new(path).is_file())
        .and_then(|path| fs::canonicalize(path).ok())
        .expect("M alias needs an existing direct IdentityFile");
    host.identity_file = Some(identity.display().to_string());

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let main = root.join("main");
    let linked = root.join("linked");
    let runtime = root.join("runtime");
    let control = root.join("control");
    let home = root.join("home");
    let ssh_dir = home.join(".ssh");
    let wrapper_bin = root.join("wrapper-bin");
    let shim_bin = root.join("git-shim-bin");
    for path in [&main, &runtime, &control, &ssh_dir, &wrapper_bin, &shim_bin] {
        fs::create_dir_all(path).unwrap();
    }
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();

    git(&main, &["init", "-q", "-b", "main"]);
    fs::write(main.join("tracked"), b"base\n").unwrap();
    git(&main, &["add", "tracked"]);
    git(&main, &["commit", "-qm", "base"]);
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    for index in 0..3 {
        fs::write(main.join("tracked"), format!("stash-{index}\n")).unwrap();
        git(
            &main,
            &["stash", "push", "-q", "-m", &format!("stash-{index}")],
        );
    }

    let ssh_config = ssh_dir.join("config");
    write_ssh_config(
        &ssh_config,
        &host.host,
        &initial,
        host.identity_file.as_deref().unwrap(),
        &initial.hostname,
    );
    let ssh_wrapper = wrapper_bin.join("ssh");
    let git_shim = shim_bin.join("git");
    install_ssh_wrapper(&ssh_wrapper);
    install_git_shim(&git_shim);

    let pid_file = control.join("ssh.pid");
    let block = control.join("block");
    let entered = control.join("entered");
    let release = control.join("release");
    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(wrapper_bin.clone()).chain(std::env::split_paths(&old_path)),
    )
    .unwrap();
    let _env = EnvGuard::set(&[
        ("HOME", home.as_os_str().to_owned()),
        ("PATH", path),
        ("KAGI_M_REAL_SSH", OsStr::new(REAL_SSH).to_owned()),
        ("KAGI_M_REAL_GIT", OsStr::new(REAL_GIT).to_owned()),
        ("KAGI_M_REMOTE_RUNTIME", runtime.as_os_str().to_owned()),
        ("KAGI_M_REMOTE_GIT_BIN", shim_bin.as_os_str().to_owned()),
        ("KAGI_M_SSH_PID_FILE", pid_file.as_os_str().to_owned()),
        ("KAGI_M_SHIM_BLOCK", block.as_os_str().to_owned()),
        ("KAGI_M_SHIM_ENTERED", entered.as_os_str().to_owned()),
        ("KAGI_M_SHIM_RELEASE", release.as_os_str().to_owned()),
    ]);

    let baseline_receipts = read_oplog_tail(100).len();
    let initial_main = invariant(&main);
    let initial_linked = invariant(&linked);

    // M2: both worktree roots resolve to one physical remote repository key.
    let mut sessions = Sessions::new();
    let main_identity = ready(&mut sessions, &host, &main, 1).repo_id;
    let linked_identity = ready(&mut sessions, &host, &linked, 1).repo_id;
    assert_eq!(main_identity, linked_identity);
    assert_eq!(
        PathBuf::from(&main_identity.common_dir),
        fs::canonicalize(main.join(".git")).unwrap()
    );

    // M3: reserve from main, prove linked-root exclusion, then drop the middle stash.
    let before = stash_oids(&main);
    assert_eq!(before.len(), 3);
    let main_ready = ready(&mut sessions, &host, &main, 1);
    assert_eq!(main_ready.selected_oid, before[1]);
    let main_job = prepare_job(&mut sessions, main_ready);
    let linked_ready = ready(&mut sessions, &host, &linked, 1);
    let linked_approved =
        approve(&mut sessions, linked_ready.token, StashPolicy::default()).unwrap();
    assert!(matches!(
        app::prepare(&mut sessions, linked_approved, LegacyBusy(false)),
        Err(AdmissionError::Busy)
    ));
    let applied = remote_stash_support::assert_remote_completion(
        &mut sessions,
        main_job.run(),
        RemoteDropOutcome::Success,
    );
    assert_eq!(
        applied.report.evidence.frame.as_ref().unwrap().selected_oid,
        before[1]
    );
    let after_middle: Vec<_> = before
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 1)
        .map(|(_, oid)| oid.clone())
        .collect();
    assert_eq!(stash_oids(&main), after_middle);
    assert_eq!(invariant(&main), initial_main);
    assert_eq!(invariant(&linked), initial_linked);
    assert_eq!(read_oplog_tail(100).len(), baseline_receipts + 1);
    assert!(!sessions.has_leases());
    assert_no_kagi_trace(&main);
    assert_no_kagi_trace(&linked);

    // M4: block immediately before the real drop, sever only the local SSH channel,
    // and prove that a before-identical read cannot issue an acknowledgement.
    fs::write(&block, b"block\n").unwrap();
    remove_if_present(&entered);
    remove_if_present(&release);
    remove_if_present(&pid_file);
    let before_unknown = stash_oids(&main);
    let invariant_before_unknown = invariant(&main);
    let unknown_ready = ready(&mut sessions, &host, &main, 0);
    let unknown_oid = unknown_ready.selected_oid.clone();
    let unknown_job = prepare_job(&mut sessions, unknown_ready);
    let mut running = BlockedRemote::start(unknown_job, release.clone());
    wait_for(&entered, "remote git shim entry");
    wait_for(&pid_file, "local ssh PID");
    assert_eq!(stash_oids(&main), before_unknown);
    assert_eq!(invariant(&main), invariant_before_unknown);
    assert!(
        !running.is_finished(),
        "remote writer must still be waiting"
    );
    let pid = fs::read_to_string(&pid_file).unwrap();
    let mut kill = Command::new("/bin/kill");
    kill.args(["-KILL", pid.trim()]);
    checked(kill, "kill local SSH channel");
    let unknown_completion = running.channel_result();
    let unknown = remote_stash_support::assert_remote_completion(
        &mut sessions,
        unknown_completion,
        RemoteDropOutcome::Unknown,
    );
    assert!(!unknown.report.evidence.stopped);
    assert!(sessions.has_leases());
    assert!(entry_to_json(unknown.report.recording.entry()).contains(&unknown_oid));
    assert_eq!(stash_oids(&main), before_unknown);
    assert_eq!(invariant(&main), invariant_before_unknown);
    assert!(app::read_reconcile(&sessions, unknown.id).is_err());
    assert!(
        sessions.has_leases(),
        "before-identical read must not acknowledge"
    );
    let linked_during_unknown = ready(&mut sessions, &host, &linked, 0);
    let linked_approved = approve(
        &mut sessions,
        linked_during_unknown.token,
        StashPolicy::default(),
    )
    .unwrap();
    assert!(matches!(
        app::prepare(&mut sessions, linked_approved, LegacyBusy(false)),
        Err(AdmissionError::NeedsReconcile)
    ));

    // M5: the surviving writer creates the external bound token only after release.
    running.release();
    wait_for(
        Path::new(&unknown.report.evidence.token_path),
        "bound completion token",
    );
    let token_metadata = fs::metadata(&unknown.report.evidence.token_path).unwrap();
    assert_eq!(token_metadata.mode() & 0o777, 0o600);
    assert_eq!(token_metadata.uid(), fs::metadata(&runtime).unwrap().uid());
    let read = app::read_reconcile(&sessions, unknown.id).expect("token + state read");
    app::acknowledge(&mut sessions, read).expect("matching read acknowledgement");
    let after_unknown: Vec<_> = before_unknown
        .iter()
        .filter(|oid| *oid != &unknown_oid)
        .cloned()
        .collect();
    assert_eq!(stash_oids(&main), after_unknown);
    assert_eq!(invariant(&main), invariant_before_unknown);
    assert_eq!(read_oplog_tail(100).len(), baseline_receipts + 2);
    assert!(!sessions.has_leases());
    remove_if_present(&block);

    // M6: change only the fixture HOME alias after plan. Re-freeze must refuse
    // before the remote drop script, then the restored direct-key profile works.
    remove_if_present(&pid_file);
    let before_drift = stash_oids(&main);
    let invariant_before_drift = invariant(&main);
    let drift_ready = ready(&mut sessions, &host, &main, 0);
    let drift_oid = drift_ready.selected_oid.clone();
    let drift_job = prepare_job(&mut sessions, drift_ready);
    write_ssh_config(
        &ssh_config,
        &host.host,
        &initial,
        host.identity_file.as_deref().unwrap(),
        "127.0.0.2",
    );
    remote_stash_support::assert_remote_completion(
        &mut sessions,
        drift_job.run(),
        RemoteDropOutcome::Refused,
    );
    assert!(
        !pid_file.exists(),
        "drift refusal must precede the drop script"
    );
    assert_eq!(stash_oids(&main), before_drift);
    assert_eq!(invariant(&main), invariant_before_drift);
    assert_eq!(read_oplog_tail(100).len(), baseline_receipts + 3);

    write_ssh_config(
        &ssh_config,
        &host.host,
        &initial,
        host.identity_file.as_deref().unwrap(),
        &initial.hostname,
    );
    let restored_ready = ready(&mut sessions, &host, &main, 0);
    assert_eq!(restored_ready.selected_oid, drift_oid);
    let restored_job = prepare_job(&mut sessions, restored_ready);
    remote_stash_support::assert_remote_completion(
        &mut sessions,
        restored_job.run(),
        RemoteDropOutcome::Success,
    );
    assert!(stash_oids(&main).is_empty());
    assert_eq!(invariant(&main), invariant_before_drift);
    assert_eq!(read_oplog_tail(100).len(), baseline_receipts + 4);
    assert!(!sessions.has_leases());
    assert_no_kagi_trace(&main);
    assert_no_kagi_trace(&linked);
}
