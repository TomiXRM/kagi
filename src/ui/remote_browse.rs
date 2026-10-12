//! Remote SSH connect + directory-browse modal (ADR-0089, Phase 1).
//!
//! Self-contained slice extracted out of `mod.rs`/`modals.rs` (the workspace is
//! deliberately split into focused view modules). It holds:
//!
//! - the modal state ([`RemoteBrowseModal`]),
//! - its renderer ([`render_remote_browse`]) — built from gpui-component
//!   inputs and the shared modal button style,
//! - the [`KagiApp`] methods that open it and drive the (background) SSH calls,
//! - the off-thread blocking helpers that call `crate::remote`.
//!
//! Everything here is **read-only** (ADR-0089): connect, list, repo-detect, HEAD
//! summary. Remote *writes* will go through the `OperationController` pipeline in
//! a later phase, never directly from here.

use super::i18n::Msg;
use gpui::{
    div, prelude::*, rgb, App, ClickEvent, Context, Entity, FocusHandle, KeyDownEvent, ListState,
    SharedString, Window,
};
use gpui_component::input::Input;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use kagi_domain::remote::{self, RemoteDirEntry, RemoteHost, RemoteRepoSummary};

use super::button_style::{modal_button, ModalButtonKind};
use super::keyboard_nav::{self, RowFocus, RowKeys, RowScroll};
use super::theme::{self, theme as current_theme};
use super::KagiApp;
use super::{button_style, dialog_a11y, e2e, list_a11y, modal_renderers};

#[path = "remote_browse_render.rs"]
mod render;
pub(crate) use render::render_remote_browse;

static REMOTE_BROWSE_GENERATION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

// ──────────────────────────────────────────────────────────────
// State
// ──────────────────────────────────────────────────────────────

/// Which stage of the remote flow the modal is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemoteBrowseStage {
    /// Entering the host / port / identity to connect to.
    Connect,
    /// Browsing a directory on the connected host.
    Browse,
}

/// Commit limit for the remote snapshot loaded into the graph (ADR-0089).
const REMOTE_SNAPSHOT_LIMIT: usize = 10_000;

/// State for the "Connect to a remote host over SSH" flow — a connection form
/// that, once connected, becomes a read-only remote directory browser that
/// detects repositories and shows the current repo's HEAD summary (ADR-0089).
///
/// All SSH work runs off the UI thread (`background_spawn`) through
/// `crate::remote`; this struct only holds the latest result to render. The
/// `*_state` fields are the real `gpui-component` inputs; the paired plain
/// `String`s are synced from them each frame (so the connect logic is decoupled
/// from the entity lifecycle, matching the other input modals).
#[derive(Clone)]
pub struct RemoteBrowseModal {
    pub(crate) generation: u64,
    pub stage: RemoteBrowseStage,
    pub host_input: String,
    pub host_state: Option<Entity<gpui_component::input::InputState>>,
    pub port_input: String,
    pub port_state: Option<Entity<gpui_component::input::InputState>>,
    pub identity_input: String,
    pub identity_state: Option<Entity<gpui_component::input::InputState>>,
    /// Connected host (set once a connection succeeds).
    pub host: Option<RemoteHost>,
    pub cwd: String,
    pub entries: Rc<[RemoteDirEntry]>,
    pub current_is_repo: bool,
    pub summary: Option<RemoteRepoSummary>,
    /// An SSH round-trip (connect / navigate / open) is in flight.
    pub busy: bool,
    pub error: Option<SharedString>,
    row_keys: RowKeys,
    row_focus: Rc<RefCell<RowFocus>>,
    row_scroll: ListState,
    selected_row: Rc<Cell<Option<usize>>>,
    focus_accepted_read: Rc<Cell<bool>>,
}

impl RemoteBrowseModal {
    pub fn new() -> Self {
        Self {
            generation: 0,
            stage: RemoteBrowseStage::Connect,
            host_input: String::new(),
            host_state: None,
            port_input: String::new(),
            port_state: None,
            identity_input: String::new(),
            identity_state: None,
            host: None,
            cwd: String::new(),
            entries: Rc::from([]),
            current_is_repo: false,
            summary: None,
            busy: false,
            error: None,
            row_keys: Rc::from([]),
            row_focus: Rc::new(RefCell::new(RowFocus::default())),
            row_scroll: ListState::new(0, gpui::ListAlignment::Top, gpui::px(0.)),
            selected_row: Rc::new(Cell::new(None)),
            focus_accepted_read: Rc::new(Cell::new(false)),
        }
    }

    fn with_generation(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }

    fn accept_directory(&mut self, data: RemoteBrowseData) {
        self.stage = RemoteBrowseStage::Browse;
        self.cwd = data.cwd;
        self.entries = data.entries.into();
        self.current_is_repo = data.is_repo;
        self.summary = data.summary;
        self.error = None;
        let parent = remote::parent_dir(&self.cwd);
        self.row_keys = parent
            .iter()
            .map(|_| format!("{}/..", self.cwd))
            .chain(
                self.entries
                    .iter()
                    .map(|entry| remote::join_path(&self.cwd, &entry.name)),
            )
            .enumerate()
            .map(|(ix, key)| (key, ix))
            .collect();
        self.row_scroll.reset(self.row_keys.len());
        *self.row_focus.borrow_mut() = RowFocus::default();
        self.selected_row
            .set((!self.row_keys.is_empty()).then_some(0));
        self.focus_accepted_read.set(true);
    }
}

impl Default for RemoteBrowseModal {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────
// KagiApp methods (open / connect / navigate)
// ──────────────────────────────────────────────────────────────

impl KagiApp {
    /// Open the "Connect to a remote host" modal (connection form).
    pub fn open_remote_browse(&mut self, cx: &mut Context<Self>) {
        self.modal_focus = Some(cx.focus_handle());
        let generation = REMOTE_BROWSE_GENERATION
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1);
        self.set_remote_browse(RemoteBrowseModal::new().with_generation(generation));
        cx.notify();
    }

    /// Close the remote browse modal without making any changes.
    pub fn cancel_remote_browse(&mut self) {
        self.clear_remote_browse();
        self.focus_root_for_modal();
    }

    /// Confirm the action presented by the current remote-browse stage.
    pub fn confirm_remote_browse(&mut self, cx: &mut Context<Self>) {
        match self.remote_browse().map(|modal| modal.stage) {
            Some(RemoteBrowseStage::Connect) => self.start_remote_connect(cx),
            Some(RemoteBrowseStage::Browse) => self.start_remote_open_repo(cx),
            None => {}
        }
    }

    /// Validate the connection form, then connect + list the home directory on a
    /// background thread (`crate::remote`). On success the modal flips to the
    /// directory browser; on failure it shows the ssh error.
    pub fn start_remote_connect(&mut self, cx: &mut Context<Self>) {
        let (host, generation) = {
            let m = match self.remote_browse_mut() {
                Some(m) => m,
                None => return,
            };
            if m.busy {
                return;
            }
            let spec = m.host_input.trim().to_string();
            let mut host = match RemoteHost::parse(&spec) {
                Some(h) => h,
                None => {
                    m.error = Some(SharedString::from(Msg::RemoteHostInvalid.t()));
                    return;
                }
            };
            let port_s = m.port_input.trim();
            if !port_s.is_empty() {
                match port_s.parse::<u16>() {
                    Ok(p) if p != 0 => host.port = Some(p),
                    _ => {
                        m.error = Some(SharedString::from(Msg::RemotePortInvalid.t()));
                        return;
                    }
                }
            }
            let id = m.identity_input.trim();
            if !id.is_empty() {
                host.identity_file = Some(id.to_string());
            }
            m.busy = true;
            m.error = None;
            m.host = Some(host.clone());
            (host, m.generation)
        };
        cx.notify();

        let connect = async move { remote_connect_blocking(&host) };
        // Scenarios supply the read a real round-trip would have returned; the
        // launch, generation guard and completion below stay production code.
        #[cfg(feature = "gui-e2e")]
        let task = e2e_transport::take_remote_connect().unwrap_or_else(|| {
            cx.background_spawn(async move {
                e2e_transport::RemoteConnectOutcome::from_transport(connect.await)
            })
        });
        #[cfg(not(feature = "gui-e2e"))]
        let task = cx.background_spawn(connect);
        cx.spawn(async move |this, acx| {
            let result = task.await;
            #[cfg(feature = "gui-e2e")]
            let result = result.into_result();
            let _ = this.update(acx, |app, cx| {
                app.update_remote_browse_from_async(generation, |m| {
                    m.busy = false;
                    match result {
                        Ok(data) => m.accept_directory(data),
                        Err(e) => m.error = Some(SharedString::from(e)),
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Navigate the remote browser into `path` (list + repo-detect + summary on
    /// a background thread).
    pub fn remote_browse_navigate(&mut self, path: String, cx: &mut Context<Self>) {
        let (host, generation) = match self.remote_browse() {
            Some(m) if m.stage == RemoteBrowseStage::Browse && !m.busy => match m.host.clone() {
                Some(host) => (host, m.generation),
                None => return,
            },
            _ => return,
        };
        if let Some(m) = self.remote_browse_mut() {
            if m.busy {
                return;
            }
            m.busy = true;
            m.error = None;
        }
        cx.notify();

        #[cfg(feature = "gui-e2e")]
        let supplied = e2e_transport::take_remote_navigate(&path);
        let browse = async move { remote_browse_blocking(&host, &path) };
        #[cfg(feature = "gui-e2e")]
        let task = supplied.unwrap_or_else(|| {
            cx.background_spawn(async move {
                e2e_transport::RemoteConnectOutcome::from_transport(browse.await)
            })
        });
        #[cfg(not(feature = "gui-e2e"))]
        let task = cx.background_spawn(browse);
        cx.spawn(async move |this, acx| {
            let result = task.await;
            #[cfg(feature = "gui-e2e")]
            let result = result.into_result();
            let _ = this.update(acx, |app, cx| {
                app.update_remote_browse_from_async(generation, |m| {
                    m.busy = false;
                    match result {
                        Ok(data) => m.accept_directory(data),
                        Err(e) => m.error = Some(SharedString::from(e)),
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Open the current remote repository read-only in the main graph/sidebar/
    /// detail views (ADR-0089 Phase 2b): load a full `RepoSnapshot` over SSH on a
    /// background thread, then hand it to [`KagiApp::enter_remote_view`] and close
    /// the modal. Read-only — no working tree, no operations.
    pub fn start_remote_open_repo(&mut self, cx: &mut Context<Self>) {
        let (host, path, generation) = match self.remote_browse() {
            Some(m) if m.current_is_repo && !m.busy => match m.host.clone() {
                Some(h) => (h, m.cwd.clone(), m.generation),
                None => return,
            },
            _ => return,
        };
        if let Some(m) = self.remote_browse_mut() {
            m.busy = true;
            m.error = None;
        }
        cx.notify();

        let view_host = host.clone();
        let open = async move { remote_open_blocking(&host, &path) };
        #[cfg(feature = "gui-e2e")]
        let task = crate::ui::e2e::take_remote_open().unwrap_or_else(|| cx.background_spawn(open));
        #[cfg(not(feature = "gui-e2e"))]
        let task = cx.background_spawn(open);
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                if !app.remote_browse_generation_is(generation) {
                    return;
                }
                match result {
                    Ok((root, snap)) => {
                        app.cancel_remote_browse();
                        app.enter_remote_view(view_host, root, snap, cx);
                    }
                    Err(e) => {
                        app.update_remote_browse_from_async(generation, |m| {
                            m.busy = false;
                            m.error = Some(SharedString::from(e));
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

// ──────────────────────────────────────────────────────────────

// Off-thread blocking helpers (call crate::remote → system `ssh`)
// ──────────────────────────────────────────────────────────────

/// One directory's worth of remote read results, gathered in a single
/// background task (listing + repo detection + optional HEAD summary).
struct RemoteBrowseData {
    cwd: String,
    entries: Vec<RemoteDirEntry>,
    is_repo: bool,
    summary: Option<RemoteRepoSummary>,
}

/// List `path`, detect whether it is a repository, and (if so) read its HEAD
/// summary — all over SSH. Returns a `String` error the UI displays.
fn remote_browse_blocking(host: &RemoteHost, path: &str) -> Result<RemoteBrowseData, String> {
    let entries = crate::remote::list_dir(host, path).map_err(|e| e.to_string())?;
    let probe = crate::remote::probe_repo(host, path).map_err(|e| e.to_string())?;
    let summary = if probe.is_repo {
        crate::remote::repo_summary(host, path).map_err(|e| e.to_string())?
    } else {
        None
    };
    Ok(RemoteBrowseData {
        cwd: path.to_string(),
        entries,
        is_repo: probe.is_repo,
        summary,
    })
}

/// Verify the connection, find the login (home) directory, and browse it.
fn remote_connect_blocking(host: &RemoteHost) -> Result<RemoteBrowseData, String> {
    crate::remote::check_connection(host).map_err(|e| e.to_string())?;
    let home = crate::remote::home_dir(host).map_err(|e| e.to_string())?;
    remote_browse_blocking(host, &home)
}

/// Load a read-only [`RepoSnapshot`](kagi_git::RepoSnapshot) of the remote repo
/// at `path` over SSH (Phase 2). Returns `(path, snapshot)`.
fn remote_open_blocking(
    host: &RemoteHost,
    path: &str,
) -> Result<(String, kagi_git::RepoSnapshot), String> {
    let snap = crate::remote::remote_snapshot(host, path, REMOTE_SNAPSHOT_LIMIT)
        .map_err(|e| e.to_string())?;
    Ok((path.to_string(), snap))
}

/// Test-only connect transport (see the module's own docs). It is a child
/// module so `RemoteBrowseData` can stay private to this file.
#[cfg(feature = "gui-e2e")]
#[path = "remote_browse_e2e.rs"]
pub mod e2e_transport;
