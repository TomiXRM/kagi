//! Remote stash-drop SSH transport (ADR-0097 / FAMILY-stash r5).
//!
//! This module alone owns process spawning and completion-token filesystem I/O.
use super::RemoteError;
use kagi_domain::plan::{OperationPlan, StateSummary};
use kagi_domain::remote::{
    self, classify_remote_drop, completion_proves_stop, encode_completion_token,
    parse_completion_token, parse_effective_ssh_config, parse_stash_frame, RemoteConnectionId,
    RemoteDropOutcome, RemoteHost, RemoteRepoId, RemoteStashFrame, RemoteStashPhase,
    RemoteStashState,
};
use kagi_git::backend::{recording, recording::Recording, ExecutionPolicy};
use kagi_git::{OpLogEntry, OpOutcome};
use sha2::{Digest, Sha256};
use std::fs;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(feature = "gui-e2e")]
mod e2e;
mod scripts;
mod security;
#[cfg(feature = "gui-e2e")]
pub use e2e::{
    note_remote_stash_e2e_refresh, remote_stash_e2e_refreshes, set_remote_stash_e2e_mode,
    RemoteStashE2eMode,
};
use scripts::{
    COMMON_DIR_SCRIPT, DROP_SCRIPT, READ_TOKEN_SCRIPT, RUNTIME_ROOT_SCRIPT, STATE_SCRIPT,
};
pub use security::verify_known_hosts_snapshot_for_test;
use security::{snapshot_known_hosts, verify_known_hosts_snapshots, FrozenFile};
#[derive(Clone, Debug)]
pub struct RemoteAttachment {
    pub session: crate::app::SessionId,
    pub host: RemoteHost,
    pub root: String,
}
#[derive(Clone, Debug)]
pub struct FrozenConnection {
    pub alias: RemoteHost,
    pub id: RemoteConnectionId,
    argv_prefix: Vec<String>,
    known_hosts_snapshots: Vec<FrozenFile>,
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
    pub runtime_root: String,
}
#[derive(Clone, Debug)]
pub struct RemoteStashEvidence {
    pub frame: Option<RemoteStashFrame>,
    pub outcome: RemoteDropOutcome,
    pub stopped: bool,
    pub transport_phase: RemoteTransportPhase,
    pub remote_job_id: String,
    pub token_path: String,
    recovery: Option<RemoteRecoveryFixture>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteTransportPhase {
    LocalSpawn,
    ScriptStarted,
    TerminalResponse,
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
type ExecutionEvidence = (Option<RemoteStashFrame>, RemoteTransportPhase, bool, String);
type ExecutionFailure = (RemoteTransportPhase, bool, String);
type ExecutionResult = Result<ExecutionEvidence, ExecutionFailure>;
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
    MalformedTerminal,
}

#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct RemotePlanFixture {
    pub connection: RemoteConnectionId,
    pub common_dir: String,
    pub before: RemoteStashState,
    pub runtime_root: String,
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
        known_hosts_snapshots: Vec::new(),
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
        runtime_root: fixture.runtime_root,
    })
}

pub fn plan_remote_stash_drop(
    attachment: RemoteAttachment,
    index: usize,
) -> Result<RemoteStashPlan, RemotePlanError> {
    #[cfg(feature = "gui-e2e")]
    if let Some(plan) = e2e::plan(attachment.clone(), index) {
        return plan;
    }
    if attachment.root.contains('\n') || attachment.root.contains('\0') {
        return Err(RemotePlanError::InvalidRepository(
            "remote root may not contain newline or NUL".into(),
        ));
    }
    let connection = freeze_connection(&attachment.host)?;
    let common_dir = probe_common_dir(&connection, &attachment.root)?;
    let before = read_state(&connection, &attachment.root)?;
    let runtime_root = probe_runtime_root(&connection)?;
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
        runtime_root,
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
    let known_hosts_snapshots = user.2.into_iter().chain(global.2).collect();
    Ok(FrozenConnection {
        alias: host.clone(),
        id,
        argv_prefix: argv,
        known_hosts_snapshots,
        _snapshot_dir: snapshot_dir,
    })
}

#[doc(hidden)]
pub fn remote_stash_drop_script_for_test() -> &'static str {
    DROP_SCRIPT
}

#[doc(hidden)]
pub fn remote_stash_read_token_script_for_test() -> &'static str {
    READ_TOKEN_SCRIPT
}
fn io_error(error: std::io::Error) -> RemotePlanError {
    RemotePlanError::Transport(error.to_string())
}

fn run_frozen(
    connection: &FrozenConnection,
    script: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<(i32, Vec<u8>, String), FrozenRunError> {
    let mut argv = connection.argv_prefix.clone();
    let mut remote = vec!["sh", "-c", script, "kagi"];
    remote.extend_from_slice(args);
    argv.push(remote::join_remote_command(&remote));
    let mut cmd = Command::new("ssh");
    cmd.args(argv).env("LC_ALL", "C");
    // The shared runner owns the child, its pipes and its deadline (#507): on
    // expiry the local ssh client is killed and reaped and we get a
    // termination-unknown stop, not a fabricated exit status.
    let run = kagi_git::cli::run_child(&mut cmd, timeout, None)
        .map_err(|e| FrozenRunError::LocalSpawn(e.to_string()))?;
    let code = match &run.status {
        Ok(code) => *code,
        Err(stop) => {
            return Err(FrozenRunError::Unconfirmed(
                RemoteError::unknown(stop).to_string(),
            ))
        }
    };
    let stderr = run.stderr_lossy();
    Ok((code, run.stdout, stderr))
}

#[derive(Debug)]
enum FrozenRunError {
    LocalSpawn(String),
    Unconfirmed(String),
}
impl std::fmt::Display for FrozenRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LocalSpawn(error) => write!(f, "ssh was not started: {error}"),
            Self::Unconfirmed(error) => {
                write!(f, "remote command termination is unconfirmed: {error}")
            }
        }
    }
}

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

fn probe_runtime_root(connection: &FrozenConnection) -> Result<String, RemotePlanError> {
    let (code, bytes, stderr) = run_frozen(connection, RUNTIME_ROOT_SCRIPT, &[], COMMAND_TIMEOUT)
        .map_err(|e| RemotePlanError::Transport(e.to_string()))?;
    if code != 0 {
        return Err(RemotePlanError::UnsafeConfig(format!(
            "remote runtime directory is unsafe: {stderr}"
        )));
    }
    let fields: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    if fields.len() != 3 || fields[0] != b"KAGI-RUNTIME" || fields[2] != b"KAGI-END\n" {
        return Err(RemotePlanError::UnsafeConfig(
            "malformed runtime-directory frame".into(),
        ));
    }
    let runtime = std::str::from_utf8(fields[1])
        .map_err(|_| RemotePlanError::UnsafeConfig("runtime path is not UTF-8".into()))?;
    if !runtime.starts_with('/') || runtime.contains('\n') || runtime.contains('\0') {
        return Err(RemotePlanError::UnsafeConfig(
            "runtime directory is not a physical absolute path".into(),
        ));
    }
    Ok(runtime.into())
}

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
    policy: ExecutionPolicy,
    fault: RemoteStashFault,
) -> RemoteStashReport {
    #[cfg(feature = "gui-e2e")]
    let fault = if let Some(mode) = e2e::mode() {
        match mode {
            RemoteStashE2eMode::Success => RemoteStashFault::Success,
            RemoteStashE2eMode::Refused => RemoteStashFault::ConfigDrift,
            RemoteStashE2eMode::Unknown => RemoteStashFault::MissingToken,
        }
    } else {
        fault
    };
    let (remote_job_id, random_error) = match new_job_id() {
        Ok(id) => (id, None),
        Err(error) => (String::new(), Some(error)),
    };
    let token_path = format!(
        "{}/kagi/remote-ops/{remote_job_id}",
        plan.runtime_root.trim_end_matches('/')
    );
    let result = if let Some(error) = random_error {
        Err((RemoteTransportPhase::LocalSpawn, true, error))
    } else {
        execute(plan, operation_id, &remote_job_id, fault)
    };
    let (frame, transport_phase, stopped, diagnostic) = match result {
        Ok(value) => value,
        Err((phase, stopped, error)) => (None, phase, stopped, error),
    };
    let script_started = transport_phase != RemoteTransportPhase::LocalSpawn;
    let outcome = classify_remote_drop(
        &plan.before,
        plan.index,
        frame.as_ref(),
        script_started,
        stopped,
    );
    let oplog_outcome = outcome_to_oplog(outcome, frame.as_ref(), &diagnostic, plan);
    let scope = format!("{}:{}", plan.attachment.host.label(), plan.attachment.root);
    let mut recorded_before = plan.preview.current.clone();
    recorded_before.dirty = format!(
        "{}; selected stash OID {}",
        recorded_before.dirty, plan.selected_oid
    );
    let mut entry = OpLogEntry::new("stash-drop", scope.clone(), recorded_before, oplog_outcome)
        .with_actor(policy.actor)
        .with_worktree(Some(scope));
    // #500: the approved stash OID as data, not only inside the `before` prose.
    entry.recovery = vec![kagi_git::oplog::RecoveryHandle::oid(
        kagi_git::oplog::recovery::STASH,
        &plan.selected_oid,
    )];
    let recording = recording::finalize(entry);
    let recovery = fake_recovery_fixture(fault, operation_id, &remote_job_id, plan);
    RemoteStashReport {
        recording,
        evidence: RemoteStashEvidence {
            frame,
            outcome,
            stopped,
            transport_phase,
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
) -> ExecutionResult {
    if fault == RemoteStashFault::LocalSpawn {
        return Err((
            RemoteTransportPhase::LocalSpawn,
            true,
            "ssh was not started".into(),
        ));
    }
    if fault != RemoteStashFault::None {
        return execute_fake(plan, fault);
    }
    let current = freeze_connection(&plan.connection.alias)
        .map_err(|e| (RemoteTransportPhase::LocalSpawn, true, e.to_string()))?;
    if current.id != plan.connection.id {
        return Ok((
            Some(refusal_frame(plan)),
            RemoteTransportPhase::TerminalResponse,
            true,
            "SSH configuration changed after plan".into(),
        ));
    }
    verify_known_hosts_snapshots(&plan.connection)
        .map_err(|error| (RemoteTransportPhase::LocalSpawn, true, error.to_string()))?;
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
        &plan.runtime_root,
    ];
    match run_frozen(&plan.connection, DROP_SCRIPT, &args, COMMAND_TIMEOUT) {
        Ok((_code, bytes, stderr)) => match parse_stash_frame(&bytes) {
            Ok(frame) => Ok((
                Some(frame),
                RemoteTransportPhase::TerminalResponse,
                true,
                stderr,
            )),
            Err(error) => Err((
                RemoteTransportPhase::ScriptStarted,
                false,
                format!("malformed terminal frame: {error:?}; {stderr}"),
            )),
        },
        Err(FrozenRunError::LocalSpawn(error)) => Err((
            RemoteTransportPhase::LocalSpawn,
            true,
            format!("ssh was not started: {error}"),
        )),
        Err(FrozenRunError::Unconfirmed(error)) => Err((
            RemoteTransportPhase::ScriptStarted,
            false,
            format!("remote command termination is unconfirmed: {error}"),
        )),
    }
}

fn execute_fake(plan: &RemoteStashPlan, fault: RemoteStashFault) -> ExecutionResult {
    if matches!(
        fault,
        RemoteStashFault::TimeoutBeforeDrop
            | RemoteStashFault::MissingToken
            | RemoteStashFault::MalformedToken
            | RemoteStashFault::WrongScopeToken
            | RemoteStashFault::UnreadableToken
            | RemoteStashFault::ValidTokenAfterTimeout
            | RemoteStashFault::MalformedTerminal
    ) {
        return Err((
            RemoteTransportPhase::ScriptStarted,
            false,
            "remote writer termination is unconfirmed".into(),
        ));
    }
    if fault == RemoteStashFault::ConfigDrift {
        return Ok((
            Some(refusal_frame(plan)),
            RemoteTransportPhase::TerminalResponse,
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
        RemoteTransportPhase::TerminalResponse,
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
    verify_known_hosts_snapshots(&plan.connection).map_err(|error| error.to_string())?;
    let (_, bytes, stderr) = run_frozen(
        &plan.connection,
        READ_TOKEN_SCRIPT,
        &[
            &plan.attachment.root,
            &plan.repo_id.common_dir,
            &plan.runtime_root,
            &evidence.token_path,
        ],
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
        RemoteStashFault::TimeoutBeforeDrop
        | RemoteStashFault::MissingToken
        | RemoteStashFault::MalformedTerminal => Err("completion token is absent".into()),
        _ => Ok(valid),
    };
    Some(RemoteRecoveryFixture {
        token,
        observed: plan.before.clone(),
        writer_alive: fault == RemoteStashFault::TimeoutBeforeDrop,
    })
}
