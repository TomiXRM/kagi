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
// Renderer
// ──────────────────────────────────────────────────────────────

/// One labelled field: a small muted label over a soft rounded box holding a
/// borderless input (the same look as Home's search field). The box is a
/// flex row so the input takes its whole width.
fn labeled_input(
    label: &'static str,
    state: Option<&Entity<gpui_component::input::InputState>>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
            div()
                .text_xs()
                .text_color(rgb(current_theme().text_muted))
                .child(SharedString::from(label)),
        )
        .children(state.map(|st| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .px_2()
                .rounded_lg()
                .bg(rgb(current_theme().bg_base))
                .border_1()
                .border_color(rgb(current_theme().surface))
                .child(Input::new(st).appearance(false).flex_1())
        }))
}

pub(crate) fn render_remote_browse(
    modal: RemoteBrowseModal,
    focus_handle: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let busy = modal.busy;
    let app = cx.entity();

    // Cancel/close: an `&mut App` handler (gpui-component Button form) that
    // closes the modal and restores root focus.
    let cancel = {
        let app = app.clone();
        move |_e: &ClickEvent, window: &mut Window, cx: &mut App| {
            app.update(cx, |this, cx| {
                this.cancel_remote_browse();
                if let Some(fh) = this.root_focus.clone() {
                    window.focus(&fh, cx);
                }
                cx.notify();
            });
        }
    };

    let mut card = div()
        .w(theme::scaled_px(520.))
        // Same popup surface as `modal_shell::modal_card` and the Settings
        // panel (`theme.panel`); `modal` is lighter and read as a different
        // surface class next to them (#454 follow-up).
        .bg(rgb(current_theme().panel))
        .rounded_xl()
        .border_1()
        .border_color(rgb(current_theme().surface))
        .shadow_lg()
        .p_6()
        .flex()
        .flex_col()
        .gap_4();

    match modal.stage {
        // ── Connect form ────────────────────────────────────────
        RemoteBrowseStage::Connect => {
            let connect = {
                let app = app.clone();
                move |_e: &ClickEvent, _w: &mut Window, cx: &mut App| {
                    app.update(cx, |this, cx| {
                        this.start_remote_connect(cx);
                        cx.notify();
                    });
                }
            };

            card = card
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_main))
                                .text_lg()
                                .child(SharedString::from(Msg::RemoteConnectTitle.t())),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(current_theme().text_muted))
                                .child(SharedString::from(Msg::RemoteSshNotice.t())),
                        ),
                )
                .child(labeled_input(
                    Msg::RemoteHostLabel.t(),
                    modal.host_state.as_ref(),
                ))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .child(div().w(theme::scaled_px(120.)).child(labeled_input(
                            Msg::RemotePortLabel.t(),
                            modal.port_state.as_ref(),
                        )))
                        .child(div().flex_1().child(labeled_input(
                            Msg::RemoteIdentityLabel.t(),
                            modal.identity_state.as_ref(),
                        ))),
                );

            if let Some(ref err) = modal.error {
                card = card.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_blocker))
                        .overflow_hidden()
                        .child(err.clone()),
                );
            }

            card = card.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .pt_2()
                    .justify_end()
                    .child(super::e2e::measure_control(
                        "remote-connect-cancel",
                        modal_button(
                            "remote-connect-cancel",
                            Msg::PlanCancel.t(),
                            ModalButtonKind::Cancel,
                            None,
                            cancel,
                            cx,
                        ),
                    ))
                    .child(super::e2e::measure_control(
                        "remote-connect-go",
                        modal_button(
                            "remote-connect-go",
                            if busy {
                                Msg::RemoteConnecting.t()
                            } else {
                                Msg::RemoteConnect.t()
                            },
                            ModalButtonKind::Primary,
                            busy.then(|| SharedString::from(Msg::RemoteRequestBusy.t())),
                            connect,
                            cx,
                        ),
                    )),
            );
        }

        // ── Directory browser ───────────────────────────────────
        RemoteBrowseStage::Browse => {
            let host_label = modal.host.as_ref().map(|h| h.label()).unwrap_or_default();

            let change_host = {
                let app = app.clone();
                move |_e: &ClickEvent, _w: &mut Window, cx: &mut App| {
                    app.update(cx, |this, cx| {
                        if let Some(m) = this.remote_browse_mut() {
                            m.stage = RemoteBrowseStage::Connect;
                            m.error = None;
                        }
                        cx.notify();
                    });
                }
            };

            // Header: host + current path + "Change host".
            card = card.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_color(rgb(current_theme().text_main))
                                    .text_lg()
                                    .child(SharedString::from(host_label)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(current_theme().text_sub))
                                    .child(SharedString::from(if busy {
                                        format!("{}  \u{2026}", modal.cwd)
                                    } else {
                                        modal.cwd.clone()
                                    })),
                            ),
                    )
                    .child(modal_button(
                        "remote-change-host",
                        Msg::RemoteChangeHost.t(),
                        ModalButtonKind::Secondary,
                        busy.then(|| SharedString::from(Msg::RemoteRequestBusy.t())),
                        change_host,
                        cx,
                    )),
            );

            // Repo card when the current directory is itself a repository.
            if modal.current_is_repo {
                let mut repo_card = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(current_theme().surface))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(current_theme().color_success))
                            .child(SharedString::from(format!(
                                "\u{25cf} {}",
                                Msg::RemoteGitRepository.t()
                            ))),
                    );
                if let Some(ref s) = modal.summary {
                    let branch = s
                        .branch
                        .clone()
                        .unwrap_or_else(|| Msg::RemoteDetached.t().to_string());
                    repo_card = repo_card
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .text_sm()
                                .child(
                                    div()
                                        .text_color(rgb(current_theme().text_main))
                                        .child(SharedString::from(branch)),
                                )
                                .child(
                                    div()
                                        .text_color(rgb(current_theme().text_sub))
                                        .child(SharedString::from(s.head_short.clone())),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(current_theme().text_muted))
                                .overflow_hidden()
                                .child(SharedString::from(s.summary.clone())),
                        );
                } else {
                    repo_card = repo_card.child(
                        div()
                            .text_xs()
                            .text_color(rgb(current_theme().text_muted))
                            .child(SharedString::from(Msg::RemoteNoCommits.t())),
                    );
                }
                let open_repo = {
                    let app = app.clone();
                    move |_e: &ClickEvent, _w: &mut Window, cx: &mut App| {
                        app.update(cx, |this, cx| {
                            this.start_remote_open_repo(cx);
                            cx.notify();
                        });
                    }
                };
                repo_card = repo_card.child(div().pt_1().child(modal_button(
                    "remote-open-repo",
                    if busy {
                        Msg::RemoteOpeningRepository.t()
                    } else {
                        Msg::RemoteOpenRepository.t()
                    },
                    ModalButtonKind::Primary,
                    busy.then(|| SharedString::from(Msg::RemoteRequestBusy.t())),
                    open_repo,
                    cx,
                )));
                card = card.child(repo_card);
            }

            card = card.child(render_directory_list(
                &modal,
                focus_handle.as_ref(),
                window,
                cx,
            ));

            if let Some(ref err) = modal.error {
                card = card.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_blocker))
                        .overflow_hidden()
                        .child(err.clone()),
                );
            }

            card = card.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .justify_end()
                    .child(modal_button(
                        "remote-browse-close",
                        Msg::RemoteClose.t(),
                        ModalButtonKind::Cancel,
                        None,
                        cancel,
                        cx,
                    )),
            );
        }
    }

    // Escape cancels (mirrors the other input modals).
    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_remote_browse();
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh, cx);
            }
            cx.stop_propagation();
            cx.notify();
        }
    });
    let focusable_card = {
        let base = div()
            .relative()
            .on_key_down(esc_cancel)
            .child(super::e2e::measure_inside("remote-browse-card"));
        if let Some(ref fh) = focus_handle {
            base.track_focus(fh).child(card)
        } else {
            base.child(card)
        }
    };

    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        // Measured from inside: a wrapper around an absolute overlay would
        // itself be laid out in flow and move the overlay with it.
        .child(super::e2e::measure_inside("active-modal/remote-browse"))
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .occlude()
                .bg(rgb(current_theme().modal_overlay))
                .opacity(0.65),
        )
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(focusable_card),
        )
}

/// Dense directory browser: RowList owns the single Tab stop, reveal and
/// keyboard ring; list_a11y owns role/name/selection. Files remain meaningful
/// selectable rows but never navigate. Busy keeps the accepted directory and
/// its selection visible and makes activation inert; failures retain that
/// same owner. Only an accepted, generation-guarded read requests row focus.
fn render_directory_list(
    modal: &RemoteBrowseModal,
    fallback: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let scroll = RowScroll::List(modal.row_scroll.clone());
    let rows = Rc::new(modal.row_focus.borrow_mut().rows(
        modal.row_keys.clone(),
        &scroll,
        fallback,
        window,
        cx,
    ));
    if modal.focus_accepted_read.replace(false)
        && !modal.row_focus.borrow().focus_first(&scroll, window, cx)
    {
        if let Some(fallback) = fallback {
            fallback.focus(window, cx);
        }
    }
    if let Some(key) = modal.row_focus.borrow().focused(window) {
        modal
            .selected_row
            .set(modal.row_keys.iter().position(|(k, _)| k == key));
    }
    let selected = modal.selected_row.get();
    let wrapper = rows.list(super::list_a11y::list_box(
        "remote-dir-list",
        div().id("remote-dir-list").w_full(),
        Msg::RemoteDirectoryList.t(),
    ));
    if modal.row_keys.is_empty() {
        return wrapper
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().text_muted))
                    .child(SharedString::from(Msg::RemoteDirectoryEmpty.t())),
            )
            .into_any_element();
    }
    let entries = modal.entries.clone();
    let cwd = modal.cwd.clone();
    let parent = remote::parent_dir(&cwd);
    let busy = modal.busy;
    let generation = modal.generation;
    let app = cx.entity();
    let row_keys = modal.row_keys.clone();
    let size = row_keys.len();
    let list = gpui::list(modal.row_scroll.clone(), move |ix, _window, _cx| {
        let (label, text, target) = if ix == 0 && parent.is_some() {
            (
                Msg::RemoteParentDirectory.t().to_string(),
                "\u{2191}  ..".to_string(),
                parent.clone(),
            )
        } else {
            let entry = &entries[ix - usize::from(parent.is_some())];
            if entry.is_dir() {
                (
                    Msg::RemoteDirectoryRow.t().replacen("{}", &entry.name, 1),
                    format!("\u{1f4c1}  {}/", entry.name),
                    Some(remote::join_path(&cwd, &entry.name)),
                )
            } else {
                (
                    Msg::RemoteFileRow.t().replacen("{}", &entry.name, 1),
                    format!("\u{1f4c4}  {}", entry.name),
                    None,
                )
            }
        };
        let navigable = target.is_some();
        let app = app.clone();
        let click_rows = rows.clone();
        let row = rows
            .row(
                ix,
                super::list_a11y::list_option(
                    "remote-dir-list",
                    div().id(SharedString::from(row_keys[ix].0.clone())),
                    ix,
                    size,
                    label,
                    selected == Some(ix),
                ),
            )
            .w_full()
            .px(keyboard_nav::inset(8.))
            .py(keyboard_nav::inset(4.))
            .rounded_sm()
            .text_sm()
            .overflow_hidden()
            .text_color(rgb(if navigable {
                current_theme().text_main
            } else {
                current_theme().text_muted
            }))
            .when(selected == Some(ix), |row| {
                row.bg(rgb(current_theme().surface))
            })
            .when(busy, |row| {
                row.aria_description(SharedString::from(Msg::RemoteRequestBusy.t()))
                    .a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
                        builder.parent_node().set_disabled();
                    })
            })
            .when(!busy && navigable, |row| {
                row.cursor_pointer()
                    .hover(|style| style.bg(rgb(current_theme().surface)))
            })
            .on_click(move |_, window, cx| {
                app.update(cx, |app, cx| {
                    if app.remote_browse_generation_is(generation) {
                        click_rows.focus_row(ix, window, cx);
                        if let Some(target) = &target {
                            app.remote_browse_navigate(target.clone(), cx);
                        }
                        cx.notify();
                    }
                });
            })
            .child(SharedString::from(text));
        #[cfg(feature = "gui-e2e")]
        let row = super::e2e::measure_control(format!("remote-dir-row-{ix}"), row);
        row.into_any_element()
    })
    .w_full()
    .h(theme::scaled_px((size as f32 * 28.).min(280.)));
    wrapper.child(list).into_any_element()
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
