//! Test-only connect transport for the Remote Browse modal (`gui-e2e` only).
//!
//! A **child** module of [`super`], not a sibling and not `ui::e2e`: the
//! connect result type stays private to its owner (a sibling would force
//! `RemoteBrowseData` to widen), and `ui::e2e` is at the repository's 800-LOC
//! file ceiling.
//!
//! Mirrors `ui::e2e::queue_remote_open`: a scenario supplies the raw stdout a
//! real SSH round-trip would have produced, and the modal's own parsers,
//! generation guard and stage transition still run. Nothing here mutates modal
//! state.

use super::RemoteBrowseData;
use kagi_domain::remote;
use std::cell::RefCell;

/// One connect round-trip's result, in the shape production consumes. Opaque:
/// scenarios build one with [`connected`] or [`refused`].
pub struct RemoteConnectOutcome(Result<RemoteBrowseData, String>);

impl RemoteConnectOutcome {
    /// Wrap what the real round-trip returned, so both transports hand the
    /// completion the same type.
    pub(super) fn from_transport(result: Result<RemoteBrowseData, String>) -> Self {
        Self(result)
    }

    pub(super) fn into_result(self) -> Result<RemoteBrowseData, String> {
        self.0
    }
}

/// A successful connect: the login directory ssh landed in, plus the stdout of
/// the same three commands `remote_browse_blocking` runs there — `ls -1ApL`,
/// `git rev-parse --is-inside-work-tree --show-toplevel`, and
/// `git log -1 --format=%h%x1f%D%x1f%s`. Production's parsers build the result.
pub fn connected(home: &str, ls: &str, probe: &str, head_log: &str) -> RemoteConnectOutcome {
    let is_repo = remote::parse_repo_probe(probe).is_repo;
    RemoteConnectOutcome(Ok(RemoteBrowseData {
        cwd: home.to_string(),
        entries: remote::parse_ls(ls),
        is_repo,
        summary: if is_repo {
            remote::parse_repo_summary(head_log)
        } else {
            None
        },
    }))
}

/// A refused connect, carrying the ssh error the form displays.
pub fn refused(error: &str) -> RemoteConnectOutcome {
    RemoteConnectOutcome(Err(error.to_string()))
}

/// The stand-in for one background connect round-trip.
pub type RemoteConnectTask = gpui::Task<RemoteConnectOutcome>;

thread_local! {
    static REMOTE_CONNECT: RefCell<Option<RemoteConnectTask>> = const { RefCell::new(None) };
}

/// Queue the result of the next connect; one round-trip at a time, like
/// `ui::e2e::queue_remote_open`.
pub fn queue_remote_connect(task: RemoteConnectTask) {
    REMOTE_CONNECT.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}

pub(super) fn take_remote_connect() -> Option<RemoteConnectTask> {
    REMOTE_CONNECT.with(|slot| slot.borrow_mut().take())
}
