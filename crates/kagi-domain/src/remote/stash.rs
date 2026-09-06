//! Pure remote stash-drop identity, wire, and outcome contracts.

/// Versioned wire contract for the remote stash writer (ADR-0097).
pub const STASH_FRAME_MAGIC: &str = "KAGI-STASH-DROP";
pub const STASH_FRAME_VERSION: &str = "1";

/// One path whose bytes participate in a frozen SSH connection identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KnownHostsIdentity {
    pub path: String,
    pub digest: String,
}

/// Security-relevant result of `ssh -G`, normalized by the pure parser.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RemoteConnectionId {
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub host_key_alias: Option<String>,
    pub identity_files: Vec<String>,
    pub certificate_files: Vec<String>,
    pub user_known_hosts: Vec<KnownHostsIdentity>,
    pub global_known_hosts: Vec<KnownHostsIdentity>,
    pub host_key_algorithms: Vec<String>,
}

/// Writer lease identity. The selected worktree root is deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RemoteRepoId {
    pub connection: RemoteConnectionId,
    pub common_dir: String,
}

/// Effective SSH values before known-host files have been read and digested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveSshConfig {
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub host_key_alias: Option<String>,
    pub identity_files: Vec<String>,
    pub certificate_files: Vec<String>,
    pub user_known_hosts_files: Vec<String>,
    pub global_known_hosts_files: Vec<String>,
    pub host_key_algorithms: Vec<String>,
}

/// Why an effective SSH profile cannot be frozen for a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshConfigError {
    Missing(&'static str),
    Malformed(&'static str),
    Unsupported(&'static str),
    UnexpandedToken(String),
}

impl std::fmt::Display for SshConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(field) => write!(f, "ssh -G omitted {field}"),
            Self::Malformed(field) => write!(f, "ssh -G returned malformed {field}"),
            Self::Unsupported(field) => write!(f, "SSH write profile uses unsupported {field}"),
            Self::UnexpandedToken(value) => {
                write!(f, "SSH path contains an unexpanded token: {value}")
            }
        }
    }
}

/// Parse and validate the security-relevant subset of OpenSSH `-G` output.
pub fn parse_effective_ssh_config(text: &str) -> Result<EffectiveSshConfig, SshConfigError> {
    let mut hostname = None;
    let mut user = None;
    let mut port = None;
    let mut host_key_alias = None;
    let mut identity_files = Vec::new();
    let mut certificate_files = Vec::new();
    let mut user_known_hosts_files = Vec::new();
    let mut global_known_hosts_files = Vec::new();
    let mut host_key_algorithms = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let value = value.trim();
        match key.to_ascii_lowercase().as_str() {
            "hostname" => hostname = Some(value.to_string()),
            "user" => user = Some(value.to_string()),
            "port" => port = value.parse::<u16>().ok(),
            "hostkeyalias" if value != "none" => host_key_alias = Some(value.to_string()),
            "identityfile" => identity_files.push(value.to_string()),
            "certificatefile" if value != "none" => certificate_files.push(value.to_string()),
            "userknownhostsfile" => {
                user_known_hosts_files.extend(value.split_whitespace().map(str::to_string))
            }
            "globalknownhostsfile" => {
                global_known_hosts_files.extend(value.split_whitespace().map(str::to_string))
            }
            "hostkeyalgorithms" => host_key_algorithms.extend(value.split(',').map(str::to_string)),
            "proxyjump" | "proxycommand" if value != "none" => {
                return Err(SshConfigError::Unsupported("proxy routing"))
            }
            "controlmaster" if value != "no" && value != "false" => {
                return Err(SshConfigError::Unsupported("ControlMaster"))
            }
            "controlpath" if value != "none" => {
                return Err(SshConfigError::Unsupported("ControlPath"))
            }
            _ => {}
        }
    }
    let config = EffectiveSshConfig {
        hostname: hostname.ok_or(SshConfigError::Missing("hostname"))?,
        user: user.ok_or(SshConfigError::Missing("user"))?,
        port: port.ok_or(SshConfigError::Malformed("port"))?,
        host_key_alias,
        identity_files,
        certificate_files,
        user_known_hosts_files,
        global_known_hosts_files,
        host_key_algorithms,
    };
    if config.identity_files.is_empty() || config.identity_files.iter().all(|p| p == "none") {
        return Err(SshConfigError::Unsupported("agent-only authentication"));
    }
    for path in config
        .certificate_files
        .iter()
        .chain(config.user_known_hosts_files.iter())
        .chain(config.global_known_hosts_files.iter())
    {
        if path.contains('%') || path.starts_with('~') || path.contains('$') {
            return Err(SshConfigError::UnexpandedToken(path.clone()));
        }
    }
    Ok(config)
}

/// Build the literal direct-SSH argv prefix from an already frozen identity.
pub fn frozen_ssh_argv(
    id: &RemoteConnectionId,
    user_known_hosts_snapshots: &[String],
    global_known_hosts_snapshots: &[String],
) -> Vec<String> {
    let mut argv = vec!["-F".into(), "/dev/null".into()];
    for value in [
        "BatchMode=yes",
        "StrictHostKeyChecking=yes",
        "CanonicalizeHostname=no",
        "ControlMaster=no",
        "ControlPath=none",
        "ProxyCommand=none",
        "IdentitiesOnly=yes",
        "IdentityAgent=none",
        "PubkeyAuthentication=yes",
        "PasswordAuthentication=no",
        "KbdInteractiveAuthentication=no",
        "GSSAPIAuthentication=no",
        "HostbasedAuthentication=no",
        "VerifyHostKeyDNS=no",
        "UpdateHostKeys=no",
    ] {
        argv.extend(["-o".into(), value.into()]);
    }
    argv.extend(["-p".into(), id.port.to_string()]);
    for path in &id.identity_files {
        argv.extend(["-i".into(), path.clone()]);
    }
    for path in &id.certificate_files {
        argv.extend(["-o".into(), format!("CertificateFile={path}")]);
    }
    argv.extend([
        "-o".into(),
        format!(
            "UserKnownHostsFile={}",
            user_known_hosts_snapshots.join(" ")
        ),
    ]);
    argv.extend([
        "-o".into(),
        format!(
            "GlobalKnownHostsFile={}",
            global_known_hosts_snapshots.join(" ")
        ),
    ]);
    argv.extend([
        "-o".into(),
        format!("HostKeyAlgorithms={}", id.host_key_algorithms.join(",")),
    ]);
    if let Some(alias) = &id.host_key_alias {
        argv.extend(["-o".into(), format!("HostKeyAlias={alias}")]);
    }
    argv.push(format!("{}@{}", id.user, id.hostname));
    argv
}

/// Repository observation transported in a fixed frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteStashState {
    pub head: String,
    pub ordered_oids: Vec<String>,
    pub index_fingerprint: String,
    pub worktree_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteStashPhase {
    Preflight,
    Dropped,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteStashFrame {
    pub phase: RemoteStashPhase,
    pub exit: i32,
    pub selected_oid: String,
    pub stdout_oid: String,
    pub before: RemoteStashState,
    pub after: RemoteStashState,
    pub error_class: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    MissingTerminator,
    FieldCount,
    Magic,
    Version,
    Value(&'static str),
}

/// Strict NUL-frame parser. Extra and missing fields are rejected alike.
pub fn parse_stash_frame(bytes: &[u8]) -> Result<RemoteStashFrame, FrameError> {
    const TERMINATOR: &[u8] = b"\0KAGI-STASH-END\n";
    let body = bytes
        .strip_suffix(TERMINATOR)
        .ok_or(FrameError::MissingTerminator)?;
    let fields: Vec<&[u8]> = body.split(|b| *b == 0).collect();
    if fields.len() != 15 {
        return Err(FrameError::FieldCount);
    }
    let field = |index: usize, name| {
        std::str::from_utf8(fields[index]).map_err(|_| FrameError::Value(name))
    };
    if field(0, "magic")? != STASH_FRAME_MAGIC {
        return Err(FrameError::Magic);
    }
    if field(1, "version")? != STASH_FRAME_VERSION {
        return Err(FrameError::Version);
    }
    let phase = match field(2, "phase")? {
        "preflight" => RemoteStashPhase::Preflight,
        "dropped" => RemoteStashPhase::Dropped,
        "complete" => RemoteStashPhase::Complete,
        _ => return Err(FrameError::Value("phase")),
    };
    let parse_oids = |value: &str| -> Result<Vec<String>, FrameError> {
        if value.is_empty() {
            return Ok(Vec::new());
        }
        value
            .split(',')
            .map(|oid| {
                valid_hex(oid, 40)
                    .then(|| oid.to_string())
                    .ok_or(FrameError::Value("oid list"))
            })
            .collect()
    };
    let selected_oid = field(4, "selected oid")?.to_string();
    let stdout_oid = field(5, "stdout oid")?.to_string();
    if !valid_hex(&selected_oid, 40) || (!stdout_oid.is_empty() && !valid_hex(&stdout_oid, 40)) {
        return Err(FrameError::Value("oid"));
    }
    Ok(RemoteStashFrame {
        phase,
        exit: field(3, "exit")?
            .parse()
            .map_err(|_| FrameError::Value("exit"))?,
        selected_oid,
        stdout_oid,
        before: RemoteStashState {
            head: field(6, "before head")?.into(),
            ordered_oids: parse_oids(field(7, "before oids")?)?,
            index_fingerprint: field(8, "before index")?.into(),
            worktree_fingerprint: field(9, "before worktree")?.into(),
        },
        after: RemoteStashState {
            head: field(10, "after head")?.into(),
            ordered_oids: parse_oids(field(11, "after oids")?)?,
            index_fingerprint: field(12, "after index")?.into(),
            worktree_fingerprint: field(13, "after worktree")?.into(),
        },
        error_class: field(14, "error class")?.into(),
    })
}

fn valid_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteDropOutcome {
    Success,
    Refused,
    Failed,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCompletionToken {
    pub operation_id: u64,
    pub remote_job_id: String,
    pub scope_digest: String,
    pub result_digest: String,
    pub result: RemoteStashFrame,
}

/// Canonical bytes hashed by `git hash-object --stdin` in the remote script.
pub fn remote_result_material(frame: &RemoteStashFrame) -> String {
    let phase = match frame.phase {
        RemoteStashPhase::Preflight => "preflight",
        RemoteStashPhase::Dropped => "dropped",
        RemoteStashPhase::Complete => "complete",
    };
    [
        STASH_FRAME_VERSION.to_string(),
        phase.into(),
        frame.exit.to_string(),
        frame.selected_oid.clone(),
        frame.stdout_oid.clone(),
        frame.before.head.clone(),
        frame.before.ordered_oids.join(","),
        frame.before.index_fingerprint.clone(),
        frame.before.worktree_fingerprint.clone(),
        frame.after.head.clone(),
        frame.after.ordered_oids.join(","),
        frame.after.index_fingerprint.clone(),
        frame.after.worktree_fingerprint.clone(),
        frame.error_class.clone(),
    ]
    .join("\n")
}

fn parse_result_material(value: &str) -> Result<RemoteStashFrame, FrameError> {
    let fields: Vec<&str> = value.split('\n').collect();
    if fields.len() != 14 || fields.iter().any(|field| field.contains('\0')) {
        return Err(FrameError::Value("result material"));
    }
    let bytes = format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0KAGI-STASH-END\n",
        STASH_FRAME_MAGIC,
        fields[0],
        fields[1],
        fields[2],
        fields[3],
        fields[4],
        fields[5],
        fields[6],
        fields[7],
        fields[8],
        fields[9],
        fields[10],
        fields[11],
        fields[12],
        fields[13],
    );
    parse_stash_frame(bytes.as_bytes())
}

/// Git's SHA-1 object id for a blob. Kept dependency-free for `kagi-domain`.
fn git_blob_digest(bytes: &[u8]) -> String {
    let mut input = format!("blob {}\0", bytes.len()).into_bytes();
    input.extend_from_slice(bytes);
    let bit_len = (input.len() as u64) * 8;
    input.push(0x80);
    while input.len() % 64 != 56 {
        input.push(0);
    }
    input.extend_from_slice(&bit_len.to_be_bytes());
    let mut h = [
        0x6745_2301_u32,
        0xefcd_ab89,
        0x98ba_dcfe,
        0x1032_5476,
        0xc3d2_e1f0,
    ];
    for chunk in input.chunks_exact(64) {
        let mut w = [0_u32; 80];
        for (index, word) in chunk.chunks_exact(4).enumerate() {
            w[index] = u32::from_be_bytes(word.try_into().expect("four-byte SHA-1 word"));
        }
        for index in 16..80 {
            w[index] = (w[index - 3] ^ w[index - 8] ^ w[index - 14] ^ w[index - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (index, word) in w.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(value);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[doc(hidden)]
pub fn encode_completion_token(
    operation_id: u64,
    remote_job_id: &str,
    scope_digest: &str,
    result: &RemoteStashFrame,
) -> Vec<u8> {
    let material = remote_result_material(result);
    let digest = git_blob_digest(material.as_bytes());
    format!(
        "KAGI-STASH-TOKEN\0{STASH_FRAME_VERSION}\0{operation_id}\0{remote_job_id}\0{scope_digest}\0{digest}\0{material}\0KAGI-TOKEN-END\n"
    )
    .into_bytes()
}

pub fn parse_completion_token(bytes: &[u8]) -> Result<RemoteCompletionToken, FrameError> {
    let body = bytes
        .strip_suffix(b"\0KAGI-TOKEN-END\n")
        .ok_or(FrameError::MissingTerminator)?;
    let fields: Vec<&[u8]> = body.split(|b| *b == 0).collect();
    if fields.len() != 7 {
        return Err(FrameError::FieldCount);
    }
    let field = |index: usize, name| {
        std::str::from_utf8(fields[index]).map_err(|_| FrameError::Value(name))
    };
    if field(0, "magic")? != "KAGI-STASH-TOKEN" {
        return Err(FrameError::Magic);
    }
    if field(1, "version")? != STASH_FRAME_VERSION {
        return Err(FrameError::Version);
    }
    let scope_digest = field(4, "scope digest")?.to_string();
    let result_digest = field(5, "result digest")?.to_string();
    if !valid_hex(&scope_digest, 64) || !valid_hex(&result_digest, 40) {
        return Err(FrameError::Value("token digest"));
    }
    let material = field(6, "result material")?;
    let result = parse_result_material(material)?;
    if git_blob_digest(material.as_bytes()) != result_digest {
        return Err(FrameError::Value("result digest"));
    }
    Ok(RemoteCompletionToken {
        operation_id: field(2, "operation id")?
            .parse()
            .map_err(|_| FrameError::Value("operation id"))?,
        remote_job_id: field(3, "remote job id")?.to_string(),
        scope_digest,
        result_digest,
        result,
    })
}

pub fn completion_proves_stop(
    token: &RemoteCompletionToken,
    operation_id: u64,
    remote_job_id: &str,
    scope_digest: &str,
) -> bool {
    token.operation_id == operation_id
        && token.remote_job_id == remote_job_id
        && token.scope_digest == scope_digest
}

/// Pure r5 outcome matrix; transport stop proof is an explicit input.
pub fn classify_remote_drop(
    planned: &RemoteStashState,
    selected_index: usize,
    frame: Option<&RemoteStashFrame>,
    script_started: bool,
    stopped: bool,
) -> RemoteDropOutcome {
    if !script_started {
        return RemoteDropOutcome::Failed;
    }
    let Some(frame) = frame else {
        return RemoteDropOutcome::Unknown;
    };
    if !stopped {
        return RemoteDropOutcome::Unknown;
    }
    if frame.phase == RemoteStashPhase::Preflight {
        return RemoteDropOutcome::Refused;
    }
    let unchanged = &frame.after == planned;
    if frame.exit != 0 && unchanged {
        return RemoteDropOutcome::Failed;
    }
    let mut expected = planned.ordered_oids.clone();
    if selected_index >= expected.len() {
        return RemoteDropOutcome::Refused;
    }
    let selected = expected.remove(selected_index);
    let verified = frame.exit == 0
        && frame.selected_oid == selected
        && frame.stdout_oid == selected
        && frame.before == *planned
        && frame.after.ordered_oids == expected
        && frame.after.head == planned.head
        && frame.after.index_fingerprint == planned.index_fingerprint
        && frame.after.worktree_fingerprint == planned.worktree_fingerprint;
    if verified {
        RemoteDropOutcome::Success
    } else if frame.phase == RemoteStashPhase::Dropped || frame.after != *planned {
        RemoteDropOutcome::Partial
    } else {
        RemoteDropOutcome::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(ch: char) -> String {
        std::iter::repeat_n(ch, 40).collect()
    }

    #[test]
    fn effective_write_config_rejects_indirection_and_agent_only() {
        let base = "hostname host\nuser alice\nport 22\nuserknownhostsfile /tmp/known\nglobalknownhostsfile /etc/ssh/ssh_known_hosts\nhostkeyalgorithms ssh-ed25519\ncontrolmaster no\ncontrolpath none\nproxyjump none\nproxycommand none\n";
        assert_eq!(
            parse_effective_ssh_config(base),
            Err(SshConfigError::Unsupported("agent-only authentication"))
        );
        assert_eq!(
            parse_effective_ssh_config(&format!("{base}identityfile /tmp/key\nproxyjump hop\n")),
            Err(SshConfigError::Unsupported("proxy routing"))
        );
        assert!(parse_effective_ssh_config(&format!("{base}identityfile /tmp/key\n")).is_ok());
        for line in [
            "proxycommand nc hop 22",
            "controlmaster auto",
            "controlpath /tmp/socket",
        ] {
            assert!(matches!(
                parse_effective_ssh_config(&format!("{base}identityfile /tmp/key\n{line}\n")),
                Err(SshConfigError::Unsupported(_))
            ));
        }
        assert!(matches!(
            parse_effective_ssh_config(&format!(
                "{base}identityfile /tmp/key\ncertificatefile %d/cert\n"
            )),
            Err(SshConfigError::UnexpandedToken(_))
        ));
    }

    #[test]
    fn frozen_direct_argv_uses_every_identity_field_without_alias_resolution() {
        let id = RemoteConnectionId {
            hostname: "192.0.2.4".into(),
            user: "alice".into(),
            port: 2222,
            host_key_alias: Some("git.example".into()),
            identity_files: vec!["/keys/id".into()],
            certificate_files: vec!["/keys/id-cert.pub".into()],
            user_known_hosts: vec![KnownHostsIdentity {
                path: "/home/a/.ssh/known_hosts".into(),
                digest: "1".repeat(64),
            }],
            global_known_hosts: vec![],
            host_key_algorithms: vec!["ssh-ed25519".into()],
        };
        let argv = frozen_ssh_argv(
            &id,
            &["/snapshot/user".into()],
            &["/snapshot/global".into()],
        );
        let joined = argv.join(" ");
        for required in [
            "-F /dev/null",
            "StrictHostKeyChecking=yes",
            "ProxyCommand=none",
            "ControlMaster=no",
            "IdentityAgent=none",
            "UserKnownHostsFile=/snapshot/user",
            "GlobalKnownHostsFile=/snapshot/global",
            "HostKeyAlias=git.example",
            "alice@192.0.2.4",
        ] {
            assert!(joined.contains(required), "missing {required} in {joined}");
        }
        assert_eq!(argv.last().map(String::as_str), Some("alice@192.0.2.4"));
        assert!(
            !argv.iter().any(|argument| argument == "git.example"),
            "the mutable alias must not be a destination"
        );
    }

    #[test]
    fn fixed_frame_and_outcome_matrix_are_exact() {
        let a = oid('a');
        let b = oid('b');
        let c = oid('c');
        let frame = [
            STASH_FRAME_MAGIC,
            STASH_FRAME_VERSION,
            "complete",
            "0",
            &b,
            &b,
            "head",
            &format!("{a},{b},{c}"),
            "index",
            "worktree",
            "head",
            &format!("{a},{c}"),
            "index",
            "worktree",
            "none",
        ]
        .join("\0")
            + "\0KAGI-STASH-END\n";
        let parsed = parse_stash_frame(frame.as_bytes()).unwrap();
        assert_eq!(
            classify_remote_drop(&parsed.before, 1, Some(&parsed), true, true),
            RemoteDropOutcome::Success
        );
        assert_eq!(
            classify_remote_drop(&parsed.before, 1, Some(&parsed), true, false),
            RemoteDropOutcome::Unknown
        );
        assert_eq!(
            parse_stash_frame(&(frame + "extra").into_bytes()),
            Err(FrameError::MissingTerminator)
        );
    }

    #[test]
    fn before_state_without_matching_completion_never_proves_stop() {
        let scope = "1".repeat(64);
        let result = RemoteStashFrame {
            phase: RemoteStashPhase::Preflight,
            exit: 1,
            selected_oid: oid('a'),
            stdout_oid: String::new(),
            before: RemoteStashState {
                head: oid('b'),
                ordered_oids: vec![oid('a')],
                index_fingerprint: oid('c'),
                worktree_fingerprint: oid('d'),
            },
            after: RemoteStashState {
                head: oid('b'),
                ordered_oids: vec![oid('a')],
                index_fingerprint: oid('c'),
                worktree_fingerprint: oid('d'),
            },
            error_class: "preflight".into(),
        };
        let raw = encode_completion_token(7, "job", &scope, &result);
        let token = parse_completion_token(&raw).unwrap();
        assert!(!completion_proves_stop(&token, 7, "other-live-job", &scope));
        assert!(completion_proves_stop(&token, 7, "job", &scope));
        let mut corrupt = raw;
        let digest = token.result_digest.as_bytes();
        let offset = corrupt
            .windows(digest.len())
            .position(|window| window == digest)
            .unwrap();
        corrupt[offset] = if corrupt[offset] == b'a' { b'b' } else { b'a' };
        assert_eq!(
            parse_completion_token(&corrupt),
            Err(FrameError::Value("result digest"))
        );
    }
}
