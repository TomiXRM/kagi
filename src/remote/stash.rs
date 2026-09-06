//! Remote stash-drop SSH transport (ADR-0097 / FAMILY-stash r5).
//!
//! This module alone owns process spawning and completion-token filesystem I/O.
use super::RemoteError;
use kagi_domain::plan::{OperationPlan, StateSummary};
use kagi_domain::remote::{
    self, classify_remote_drop, completion_proves_stop, encode_completion_token,
    parse_completion_token, parse_effective_ssh_config, parse_stash_frame, KnownHostsIdentity,
    RemoteConnectionId, RemoteDropOutcome, RemoteHost, RemoteRepoId, RemoteStashFrame,
    RemoteStashPhase, RemoteStashState,
};
use kagi_git::backend::recording::Recording;
use kagi_git::{Actor, OpLogEntry, OpOutcome};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc};
use std::time::Duration;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
#[derive(Clone, Debug)]
pub struct RemoteAttachment {
    pub host: RemoteHost,
    pub root: String,
    pub generation: u64,
}
#[derive(Clone, Debug)]
pub struct FrozenConnection {
    pub alias: RemoteHost,
    pub id: RemoteConnectionId,
    argv_prefix: Vec<String>,
    _snapshot_dir: Arc<tempfile::TempDir>,
}
#[derive(Clone, Debug)]
pub struct RemoteStashPlan {
    pub preview: Arc<OperationPlan>,
    pub attachment: RemoteAttachment,
    pub connection: FrozenConnection,
    pub repo_id: RemoteRepoId,
    pub before: RemoteStashState,
    pub index: usize,
    pub selected_oid: String,
}
#[derive(Clone, Debug)]
pub struct RemoteStashEvidence {
    pub frame: Option<RemoteStashFrame>,
    pub outcome: RemoteDropOutcome,
    pub stopped: bool,
    pub remote_job_id: String,
    pub token_path: String,
    recovery: Option<RemoteRecoveryFixture>,
}
#[derive(Clone, Debug)]
struct RemoteRecoveryFixture {
    token: Result<Vec<u8>, String>,
    observed: RemoteStashState,
    writer_alive: bool,
}

#[derive(Clone, Debug)]
pub struct RemoteStashReport {
    pub recording: Recording,
    pub evidence: RemoteStashEvidence,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemotePlanError {
    Transport(String),
    UnsafeConfig(String),
    InvalidRepository(String),
    InvalidState(String),
}
impl std::fmt::Display for RemotePlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e)
            | Self::UnsafeConfig(e)
            | Self::InvalidRepository(e)
            | Self::InvalidState(e) => f.write_str(e),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub enum RemoteStashFault {
    None,
    Success,
    LocalSpawn,
    ConfigDrift,
    PreflightDrift,
    NonZeroUnchanged,
    VerifyMismatch,
    TimeoutBeforeDrop,
    MissingToken,
    MalformedToken,
    WrongScopeToken,
    UnreadableToken,
    ValidTokenAfterTimeout,
}

#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct RemotePlanFixture {
    pub connection: RemoteConnectionId,
    pub common_dir: String,
    pub before: RemoteStashState,
}

#[doc(hidden)]
pub fn plan_remote_stash_drop_for_test(
    attachment: RemoteAttachment,
    index: usize,
    fixture: RemotePlanFixture,
) -> Result<RemoteStashPlan, RemotePlanError> {
    let selected_oid = fixture
        .before
        .ordered_oids
        .get(index)
        .cloned()
        .ok_or_else(|| {
            RemotePlanError::InvalidState(format!("stash@{{{index}}} no longer exists"))
        })?;
    let label = format!("stash@{{{index}}}: {selected_oid}");
    let preview = Arc::new(kagi_git::plan_stash_drop_remote(
        &label,
        fixture.before.head.clone(),
    ));
    let temp = Arc::new(tempfile::tempdir().map_err(io_error)?);
    let frozen = FrozenConnection {
        alias: attachment.host.clone(),
        id: fixture.connection.clone(),
        argv_prefix: Vec::new(),
        _snapshot_dir: temp,
    };
    Ok(RemoteStashPlan {
        preview,
        attachment,
        connection: frozen,
        repo_id: RemoteRepoId {
            connection: fixture.connection,
            common_dir: fixture.common_dir,
        },
        before: fixture.before,
        index,
        selected_oid,
    })
}

pub fn plan_remote_stash_drop(
    attachment: RemoteAttachment,
    index: usize,
) -> Result<RemoteStashPlan, RemotePlanError> {
    if attachment.root.contains('\n') || attachment.root.contains('\0') {
        return Err(RemotePlanError::InvalidRepository(
            "remote root may not contain newline or NUL".into(),
        ));
    }
    let connection = freeze_connection(&attachment.host)?;
    let common_dir = probe_common_dir(&connection, &attachment.root)?;
    let before = read_state(&connection, &attachment.root)?;
    let selected_oid = before.ordered_oids.get(index).cloned().ok_or_else(|| {
        RemotePlanError::InvalidState(format!("stash@{{{index}}} no longer exists"))
    })?;
    let label = format!("stash@{{{index}}}: {selected_oid}");
    let preview = Arc::new(kagi_git::plan_stash_drop_remote(
        &label,
        before.head.clone(),
    ));
    let repo_id = RemoteRepoId {
        connection: connection.id.clone(),
        common_dir,
    };
    Ok(RemoteStashPlan {
        preview,
        attachment,
        connection,
        repo_id,
        before,
        index,
        selected_oid,
    })
}

fn freeze_connection(host: &RemoteHost) -> Result<FrozenConnection, RemotePlanError> {
    if host.identity_file.is_none() {
        return Err(RemotePlanError::UnsafeConfig(
            "remote writes require an explicit identity file; ssh-agent-only profiles stay read-only".into(),
        ));
    }
    let target = host.target();
    let mut config_args = vec!["-G".to_string()];
    config_args.extend(host.connection_opts());
    config_args.extend(["--".into(), target]);
    let output = Command::new("ssh")
        .args(config_args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| RemotePlanError::Transport(format!("failed to run ssh -G: {e}")))?;
    if !output.status.success() {
        return Err(RemotePlanError::Transport(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| RemotePlanError::UnsafeConfig("ssh -G output is not UTF-8".into()))?;
    let mut effective = parse_effective_ssh_config(&text)
        .map_err(|e| RemotePlanError::UnsafeConfig(e.to_string()))?;
    if let Some(identity) = &host.identity_file {
        let identity = fs::canonicalize(identity).map_err(io_error)?;
        effective.identity_files = vec![identity.display().to_string()];
    }
    let snapshot_dir = Arc::new(tempfile::tempdir().map_err(io_error)?);
    let user = snapshot_known_hosts(
        &effective.user_known_hosts_files,
        snapshot_dir.path(),
        "user",
    )?;
    let global = snapshot_known_hosts(
        &effective.global_known_hosts_files,
        snapshot_dir.path(),
        "global",
    )?;
    let id = RemoteConnectionId {
        hostname: effective.hostname.clone(),
        user: effective.user.clone(),
        port: effective.port,
        host_key_alias: effective.host_key_alias.clone(),
        identity_files: effective.identity_files.clone(),
        certificate_files: effective.certificate_files.clone(),
        user_known_hosts: user.0,
        global_known_hosts: global.0,
        host_key_algorithms: effective.host_key_algorithms.clone(),
    };
    let argv = remote::frozen_ssh_argv(&id, &user.1, &global.1);
    Ok(FrozenConnection {
        alias: host.clone(),
        id,
        argv_prefix: argv,
        _snapshot_dir: snapshot_dir,
    })
}

fn snapshot_known_hosts(
    paths: &[String],
    dir: &Path,
    prefix: &str,
) -> Result<(Vec<KnownHostsIdentity>, Vec<String>), RemotePlanError> {
    let mut identities = Vec::new();
    let mut snapshots = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if path == "none" {
            continue;
        }
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(io_error(error)),
        };
        let digest = hex_digest(&bytes);
        let snapshot = dir.join(format!("{prefix}-{index}"));
        fs::write(&snapshot, &bytes).map_err(io_error)?;
        set_owner_only(&snapshot)?;
        identities.push(KnownHostsIdentity {
            path: path.clone(),
            digest,
        });
        snapshots.push(snapshot.display().to_string());
    }
    if snapshots.is_empty() {
        return Err(RemotePlanError::UnsafeConfig(
            "no known_hosts file can be frozen".into(),
        ));
    }
    Ok((identities, snapshots))
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<(), RemotePlanError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(io_error)
}
#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> Result<(), RemotePlanError> {
    Ok(())
}
fn io_error(error: std::io::Error) -> RemotePlanError {
    RemotePlanError::Transport(error.to_string())
}

fn run_frozen(
    connection: &FrozenConnection,
    script: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<(i32, Vec<u8>, String), RemoteError> {
    let mut argv = connection.argv_prefix.clone();
    let mut remote = vec!["sh", "-c", script, "kagi"];
    remote.extend_from_slice(args);
    argv.push(remote::join_remote_command(&remote));
    let child = Command::new("ssh")
        .args(argv)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| RemoteError::Spawn(e.to_string()))?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(timeout)
        .map_err(|_| RemoteError::Timeout)?
        .map_err(|e| RemoteError::Spawn(e.to_string()))?;
    Ok((
        output.status.code().unwrap_or(-1),
        output.stdout,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

const COMMON_DIR_SCRIPT: &str = r#"set -eu
root=$1
cd -P -- "$root"
root=$(pwd -P)
test "$(git rev-parse --is-inside-work-tree)" = true
common=$(git rev-parse --path-format=absolute --git-common-dir)
cd -P -- "$common"
printf 'KAGI-COMMON-DIR\0%s\0KAGI-END\n' "$(pwd -P)""#;

fn probe_common_dir(connection: &FrozenConnection, root: &str) -> Result<String, RemotePlanError> {
    let (code, bytes, stderr) = run_frozen(connection, COMMON_DIR_SCRIPT, &[root], COMMAND_TIMEOUT)
        .map_err(|e| RemotePlanError::Transport(e.to_string()))?;
    if code != 0 {
        return Err(RemotePlanError::InvalidRepository(stderr));
    }
    let fields: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    if fields.len() != 3 || fields[0] != b"KAGI-COMMON-DIR" || fields[2] != b"KAGI-END\n" {
        return Err(RemotePlanError::InvalidRepository(
            "malformed common-dir frame".into(),
        ));
    }
    let common = std::str::from_utf8(fields[1])
        .map_err(|_| RemotePlanError::InvalidRepository("common-dir is not UTF-8".into()))?;
    if !common.starts_with('/') || common.contains('\n') {
        return Err(RemotePlanError::InvalidRepository(
            "common-dir is not a physical absolute path".into(),
        ));
    }
    Ok(common.into())
}

const STATE_SCRIPT: &str = r#"set -eu
root=$1
cd -P -- "$root"
head=$(git rev-parse --verify HEAD)
oids=$(git stash list --format=%H | paste -sd, -)
index_path=$(git rev-parse --git-path index)
if test -f "$index_path"; then index=$(git hash-object "$index_path"); else index=missing; fi
worktree=$(git status --porcelain=v2 -z --untracked-files=all | git hash-object --stdin)
printf 'KAGI-STATE\0%s\0%s\0%s\0%s\0KAGI-END\n' "$head" "$oids" "$index" "$worktree""#;

fn read_state(
    connection: &FrozenConnection,
    root: &str,
) -> Result<RemoteStashState, RemotePlanError> {
    let (code, bytes, stderr) = run_frozen(connection, STATE_SCRIPT, &[root], COMMAND_TIMEOUT)
        .map_err(|e| RemotePlanError::Transport(e.to_string()))?;
    if code != 0 {
        return Err(RemotePlanError::InvalidState(stderr));
    }
    let fields: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    if fields.len() != 6 || fields[0] != b"KAGI-STATE" || fields[5] != b"KAGI-END\n" {
        return Err(RemotePlanError::InvalidState(
            "malformed state frame".into(),
        ));
    }
    let text = |i| {
        std::str::from_utf8(fields[i])
            .map_err(|_| RemotePlanError::InvalidState("state frame is not UTF-8".into()))
    };
    let ordered_oids = if text(2)?.is_empty() {
        Vec::new()
    } else {
        text(2)?.split(',').map(str::to_string).collect()
    };
    if ordered_oids
        .iter()
        .any(|oid| oid.len() != 40 || !oid.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(RemotePlanError::InvalidState(
            "state frame contains an invalid OID".into(),
        ));
    }
    Ok(RemoteStashState {
        head: text(1)?.into(),
        ordered_oids,
        index_fingerprint: text(3)?.into(),
        worktree_fingerprint: text(4)?.into(),
    })
}

pub fn run_remote_stash_drop(
    plan: &RemoteStashPlan,
    operation_id: u64,
    actor: Actor,
    fault: RemoteStashFault,
) -> RemoteStashReport {
    let (remote_job_id, random_error) = match new_job_id() {
        Ok(id) => (id, None),
        Err(error) => (String::new(), Some(error)),
    };
    let token_path = format!("$XDG_RUNTIME_DIR/kagi/remote-ops/{remote_job_id}");
    let result = if let Some(error) = random_error {
        Err((false, true, error))
    } else {
        execute(plan, operation_id, &remote_job_id, fault)
    };
    let (frame, script_started, stopped, diagnostic) = match result {
        Ok(value) => value,
        Err((started, stopped, error)) => (None, started, stopped, error),
    };
    let outcome = classify_remote_drop(
        &plan.before,
        plan.index,
        frame.as_ref(),
        script_started,
        stopped,
    );
    let oplog_outcome = outcome_to_oplog(outcome, frame.as_ref(), &diagnostic, plan);
    let scope = format!("{}:{}", plan.attachment.host.label(), plan.attachment.root);
    let entry = OpLogEntry::new(
        "stash-drop",
        scope.clone(),
        plan.preview.current.clone(),
        oplog_outcome,
    )
    .with_actor(actor)
    .with_worktree(Some(scope));
    let recording = match kagi_git::oplog::append_oplog_receipt(&entry) {
        Ok((path, entry)) => Recording::Appended { path, entry },
        Err(error) => Recording::Failed {
            attempted: entry,
            error: error.to_string(),
        },
    };
    let recovery = fake_recovery_fixture(fault, operation_id, &remote_job_id, plan);
    RemoteStashReport {
        recording,
        evidence: RemoteStashEvidence {
            frame,
            outcome,
            stopped,
            remote_job_id,
            token_path,
            recovery,
        },
    }
}

fn execute(
    plan: &RemoteStashPlan,
    operation_id: u64,
    remote_job_id: &str,
    fault: RemoteStashFault,
) -> Result<(Option<RemoteStashFrame>, bool, bool, String), (bool, bool, String)> {
    if fault == RemoteStashFault::LocalSpawn {
        return Err((false, true, "ssh was not started".into()));
    }
    if fault != RemoteStashFault::None {
        return execute_fake(plan, fault);
    }
    let current =
        freeze_connection(&plan.connection.alias).map_err(|e| (false, true, e.to_string()))?;
    if current.id != plan.connection.id {
        return Ok((
            Some(refusal_frame(plan)),
            true,
            true,
            "SSH configuration changed after plan".into(),
        ));
    }
    let operation = operation_id.to_string();
    let index = plan.index.to_string();
    let before_oids = plan.before.ordered_oids.join(",");
    let scope_digest = scope_digest(&plan.repo_id);
    let args = [
        &plan.attachment.root[..],
        &plan.repo_id.common_dir,
        &operation,
        remote_job_id,
        &index,
        &plan.selected_oid,
        &plan.before.head,
        &before_oids,
        &plan.before.index_fingerprint,
        &plan.before.worktree_fingerprint,
        &scope_digest,
    ];
    match run_frozen(&plan.connection, DROP_SCRIPT, &args, COMMAND_TIMEOUT) {
        Ok((_code, bytes, stderr)) => match parse_stash_frame(&bytes) {
            Ok(frame) => Ok((Some(frame), true, true, stderr)),
            Err(error) => Err((
                true,
                true,
                format!("malformed terminal frame: {error:?}; {stderr}"),
            )),
        },
        Err(error) => Err((true, false, error.to_string())),
    }
}

fn execute_fake(
    plan: &RemoteStashPlan,
    fault: RemoteStashFault,
) -> Result<(Option<RemoteStashFrame>, bool, bool, String), (bool, bool, String)> {
    if matches!(
        fault,
        RemoteStashFault::TimeoutBeforeDrop
            | RemoteStashFault::MissingToken
            | RemoteStashFault::MalformedToken
            | RemoteStashFault::WrongScopeToken
            | RemoteStashFault::UnreadableToken
            | RemoteStashFault::ValidTokenAfterTimeout
    ) {
        return Err((
            true,
            false,
            "remote writer termination is unconfirmed".into(),
        ));
    }
    if fault == RemoteStashFault::ConfigDrift {
        return Ok((
            Some(refusal_frame(plan)),
            true,
            true,
            "SSH configuration changed after plan".into(),
        ));
    }
    let mut after = plan.before.clone();
    let (phase, exit, stdout) = match fault {
        RemoteStashFault::Success => {
            after.ordered_oids.remove(plan.index);
            (RemoteStashPhase::Complete, 0, plan.selected_oid.clone())
        }
        RemoteStashFault::PreflightDrift => (RemoteStashPhase::Preflight, 1, String::new()),
        RemoteStashFault::NonZeroUnchanged => (RemoteStashPhase::Complete, 1, String::new()),
        RemoteStashFault::VerifyMismatch => {
            after.ordered_oids.remove(plan.index);
            after.worktree_fingerprint = "changed".into();
            (RemoteStashPhase::Dropped, 0, plan.selected_oid.clone())
        }
        _ => unreachable!(),
    };
    Ok((
        Some(RemoteStashFrame {
            phase,
            exit,
            selected_oid: plan.selected_oid.clone(),
            stdout_oid: stdout,
            before: plan.before.clone(),
            after,
            error_class: "fake".into(),
        }),
        true,
        true,
        String::new(),
    ))
}

fn refusal_frame(plan: &RemoteStashPlan) -> RemoteStashFrame {
    RemoteStashFrame {
        phase: RemoteStashPhase::Preflight,
        exit: 1,
        selected_oid: plan.selected_oid.clone(),
        stdout_oid: String::new(),
        before: plan.before.clone(),
        after: plan.before.clone(),
        error_class: "preflight".into(),
    }
}

const DROP_SCRIPT: &str = r#"set -eu
root=$1; expected_common=$2; operation=$3; job=$4; index=$5; selected=$6
expected_head=$7; expected_oids=$8; expected_index=$9; shift 9; expected_worktree=$1; scope_digest=$2
cd -P -- "$root"
common=$(git rev-parse --path-format=absolute --git-common-dir)
cd -P -- "$common"; common=$(pwd -P); cd -P -- "$root"
state() {
  head=$(git rev-parse --verify HEAD)
  oids=$(git stash list --format=%H | paste -sd, -)
  index_path=$(git rev-parse --git-path index)
  if test -f "$index_path"; then index_hash=$(git hash-object "$index_path"); else index_hash=missing; fi
  worktree=$(git status --porcelain=v2 -z --untracked-files=all | git hash-object --stdin)
}
state
before_head=$head; before_oids=$oids; before_index=$index_hash; before_worktree=$worktree
refuse() {
  printf 'KAGI-STASH-DROP\0%s\0preflight\0%s\0%s\0\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0preflight\0KAGI-STASH-END\n' \
    1 1 "$selected" "$before_head" "$before_oids" "$before_index" "$before_worktree" \
    "$before_head" "$before_oids" "$before_index" "$before_worktree"
  exit 0
}
if test "$common" != "$expected_common" || test "$head" != "$expected_head" ||
   test "$oids" != "$expected_oids" || test "$index_hash" != "$expected_index" ||
   test "$worktree" != "$expected_worktree"; then
  refuse
fi
runtime=${XDG_RUNTIME_DIR:-$HOME/.cache}
mkdir -p -m 700 "$runtime/kagi/remote-ops" || refuse
test ! -L "$runtime/kagi" && test ! -L "$runtime/kagi/remote-ops" || refuse
runtime=$(cd -P -- "$runtime" && pwd -P) || refuse
ops="$runtime/kagi/remote-ops"
case "$ops/" in "$root/"*|"$common/"*) refuse;; esac
owner=$(stat -f %u "$ops" 2>/dev/null || stat -c %u "$ops" 2>/dev/null) || refuse
mode=$(stat -f %Lp "$ops" 2>/dev/null || stat -c %a "$ops" 2>/dev/null) || refuse
test "$owner" = "$(id -u)" && test "$mode" = 700 || refuse
drop_out=$(git stash drop "stash@{$index}" 2>&1) && exit_code=0 || exit_code=$?
stdout_oid=${drop_out##*' ('}; stdout_oid=${stdout_oid%')'}
state
material=$(printf '%s\n' 1 complete "$exit_code" "$selected" "$stdout_oid" "$before_head" "$before_oids" "$before_index" \
  "$before_worktree" "$head" "$oids" "$index_hash" "$worktree" git)
digest=$(printf %s "$material" | git hash-object --stdin)
token="$ops/$job"; tmp="$token.tmp"
printf 'KAGI-STASH-TOKEN\0%s\0%s\0%s\0%s\0%s\0%s\0KAGI-TOKEN-END\n' \
  1 "$operation" "$job" "$scope_digest" "$digest" "$material" >"$tmp"
chmod 600 "$tmp"; mv "$tmp" "$token"
printf 'KAGI-STASH-DROP\0%s\0complete\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0git\0KAGI-STASH-END\n' \
  1 "$exit_code" "$selected" "$stdout_oid" "$before_head" "$before_oids" "$before_index" \
  "$before_worktree" "$head" "$oids" "$index_hash" "$worktree"
exit "$exit_code""#;

fn outcome_to_oplog(
    outcome: RemoteDropOutcome,
    frame: Option<&RemoteStashFrame>,
    diagnostic: &str,
    plan: &RemoteStashPlan,
) -> OpOutcome {
    let after = frame
        .map(|f| StateSummary {
            head: f.after.head.clone(),
            dirty: format!("stash OIDs: {}", f.after.ordered_oids.join(",")),
        })
        .unwrap_or_else(|| plan.preview.current.clone());
    match outcome {
        RemoteDropOutcome::Success => OpOutcome::Success { after },
        RemoteDropOutcome::Refused => OpOutcome::Refused {
            blockers: vec![if diagnostic.is_empty() {
                "remote state changed after plan".into()
            } else {
                diagnostic.into()
            }],
        },
        RemoteDropOutcome::Failed => OpOutcome::Failed {
            error: if diagnostic.is_empty() {
                "remote stash drop failed without mutation".into()
            } else {
                diagnostic.into()
            },
        },
        RemoteDropOutcome::Partial => OpOutcome::Partial {
            after,
            error: if diagnostic.is_empty() {
                "remote stash changed but verification failed".into()
            } else {
                diagnostic.into()
            },
        },
        RemoteDropOutcome::Unknown => OpOutcome::Unknown {
            after,
            evidence: if diagnostic.is_empty() {
                "remote operation termination is unconfirmed; do not resend".into()
            } else {
                format!("{diagnostic}; do not resend")
            },
        },
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn new_job_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| format!("cannot create remote job id: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

const READ_TOKEN_SCRIPT: &str = r#"set -eu
job=$1
runtime=${XDG_RUNTIME_DIR:-$HOME/.cache}
token="$runtime/kagi/remote-ops/$job"
test -f "$token"; test ! -L "$token"; cat -- "$token""#;

pub fn reconcile_remote_stash(
    plan: &RemoteStashPlan,
    evidence: &RemoteStashEvidence,
    operation_id: u64,
) -> Result<String, String> {
    if let Some(fixture) = &evidence.recovery {
        if fixture.writer_alive {
            return Err(
                "old remote writer may still be alive; acknowledgement was not issued".into(),
            );
        }
        let bytes = fixture.token.as_ref().map_err(Clone::clone)?;
        let token = parse_completion_token(bytes)
            .map_err(|e| format!("completion token is invalid: {e:?}"))?;
        let scope = scope_digest(&plan.repo_id);
        if !completion_proves_stop(&token, operation_id, &evidence.remote_job_id, &scope) {
            return Err("completion token does not match the operation and remote scope".into());
        }
        return Ok(format!(
            "remote writer stopped; HEAD {}; stash count {}",
            fixture.observed.head,
            fixture.observed.ordered_oids.len()
        ));
    }
    let (_, bytes, stderr) = run_frozen(
        &plan.connection,
        READ_TOKEN_SCRIPT,
        &[&evidence.remote_job_id],
        COMMAND_TIMEOUT,
    )
    .map_err(|e| e.to_string())?;
    let token = parse_completion_token(&bytes)
        .map_err(|e| format!("completion token is invalid: {e:?}; {stderr}"))?;
    let scope = scope_digest(&plan.repo_id);
    if !completion_proves_stop(&token, operation_id, &evidence.remote_job_id, &scope) {
        return Err("completion token does not match the operation and remote scope".into());
    }
    let state = read_state(&plan.connection, &plan.attachment.root).map_err(|e| e.to_string())?;
    Ok(format!(
        "remote writer stopped; HEAD {}; stash count {}",
        state.head,
        state.ordered_oids.len()
    ))
}

fn scope_digest(repo: &RemoteRepoId) -> String {
    hex_digest(format!("{:?}:{}", repo.connection, repo.common_dir).as_bytes())
}

fn fake_recovery_fixture(
    fault: RemoteStashFault,
    operation_id: u64,
    remote_job_id: &str,
    plan: &RemoteStashPlan,
) -> Option<RemoteRecoveryFixture> {
    let relevant = matches!(
        fault,
        RemoteStashFault::TimeoutBeforeDrop
            | RemoteStashFault::MissingToken
            | RemoteStashFault::MalformedToken
            | RemoteStashFault::WrongScopeToken
            | RemoteStashFault::UnreadableToken
            | RemoteStashFault::ValidTokenAfterTimeout
    );
    if !relevant {
        return None;
    }
    let scope = if fault == RemoteStashFault::WrongScopeToken {
        "0".repeat(64)
    } else {
        scope_digest(&plan.repo_id)
    };
    let valid = encode_completion_token(operation_id, remote_job_id, &scope, &refusal_frame(plan));
    let token = match fault {
        RemoteStashFault::MalformedToken => Ok(b"broken".to_vec()),
        RemoteStashFault::UnreadableToken => Err("completion token is unreadable".into()),
        RemoteStashFault::TimeoutBeforeDrop | RemoteStashFault::MissingToken => {
            Err("completion token is absent".into())
        }
        _ => Ok(valid),
    };
    Some(RemoteRecoveryFixture {
        token,
        observed: plan.before.clone(),
        writer_alive: fault == RemoteStashFault::TimeoutBeforeDrop,
    })
}
