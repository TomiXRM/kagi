//! Finite GUI-E2E transport. It exercises the real app/UI adapter without SSH.
use super::*;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub enum RemoteStashE2eMode {
    Success,
    Refused,
    Unknown,
}

#[derive(Default)]
struct State {
    mode: Option<RemoteStashE2eMode>,
    refreshes: usize,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

#[doc(hidden)]
pub fn set_remote_stash_e2e_mode(mode: Option<RemoteStashE2eMode>) {
    *state().lock().expect("remote stash E2E state") = State { mode, refreshes: 0 };
}

#[doc(hidden)]
pub fn note_remote_stash_e2e_refresh() -> bool {
    let mut state = state().lock().expect("remote stash E2E state");
    if state.mode.is_none() {
        return false;
    }
    state.refreshes += 1;
    true
}

#[doc(hidden)]
pub fn remote_stash_e2e_refreshes() -> usize {
    state().lock().expect("remote stash E2E state").refreshes
}

pub(super) fn mode() -> Option<RemoteStashE2eMode> {
    state().lock().expect("remote stash E2E state").mode
}

pub(super) fn plan(
    attachment: RemoteAttachment,
    index: usize,
) -> Option<Result<RemoteStashPlan, RemotePlanError>> {
    mode()?;
    let oid = |byte: u8| std::iter::repeat_n(char::from(byte), 40).collect();
    Some(plan_remote_stash_drop_for_test(
        attachment,
        index,
        RemotePlanFixture {
            connection: RemoteConnectionId {
                hostname: "e2e.invalid".into(),
                user: "kagi-e2e".into(),
                port: 22,
                host_key_alias: None,
                identity_files: vec!["/e2e/id".into()],
                certificate_files: Vec::new(),
                user_known_hosts: vec![KnownHostsIdentity {
                    path: "/e2e/known_hosts".into(),
                    digest: "1".repeat(64),
                }],
                global_known_hosts: Vec::new(),
                host_key_algorithms: vec!["ssh-ed25519".into()],
            },
            common_dir: "/srv/repo/.git".into(),
            before: RemoteStashState {
                head: oid(b'f'),
                ordered_oids: vec![oid(b'a'), oid(b'b'), oid(b'c')],
                index_fingerprint: oid(b'd'),
                worktree_fingerprint: oid(b'e'),
            },
        },
    ))
}
