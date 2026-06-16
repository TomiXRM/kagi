//! Remote SSH connect + directory-browse modal (ADR-0089, Phase 1).
//!
//! Self-contained slice extracted out of `mod.rs`/`modals.rs` (the workspace is
//! deliberately split into focused view modules). It holds:
//!
//! - the modal state ([`RemoteBrowseModal`]),
//! - its renderer ([`render_remote_browse_modal`]) — built from the
//!   **longbridge `gpui-component`** widgets already vendored as a dependency
//!   (`Button`, `Input`) rather than hand-rolled `div`s, so the dialog matches
//!   the rest of the component-based UI,
//! - the [`KagiApp`] methods that open it and drive the (background) SSH calls,
//! - the off-thread blocking helpers that call `kagi::remote`.
//!
//! Everything here is **read-only** (ADR-0089): connect, list, repo-detect, HEAD
//! summary. Remote *writes* will go through the `OperationController` pipeline in
//! a later phase, never directly from here.

use gpui::{
    div, prelude::*, rgb, App, ClickEvent, Context, Entity, FocusHandle, KeyDownEvent,
    SharedString, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::{Disableable as _, Sizable as _};

use kagi_domain::remote::{self, RemoteDirEntry, RemoteHost, RemoteRepoSummary};

use super::theme::{self, theme as current_theme};
use super::KagiApp;

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

/// State for the "Connect to a remote host over SSH" flow — a connection form
/// that, once connected, becomes a read-only remote directory browser that
/// detects repositories and shows the current repo's HEAD summary (ADR-0089).
///
/// All SSH work runs off the UI thread (`background_spawn`) through
/// `kagi::remote`; this struct only holds the latest result to render. The
/// `*_state` fields are the real `gpui-component` inputs; the paired plain
/// `String`s are synced from them each frame (so the connect logic is decoupled
/// from the entity lifecycle, matching the other input modals).
#[derive(Clone)]
pub struct RemoteBrowseModal {
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
    pub entries: Vec<RemoteDirEntry>,
    pub current_is_repo: bool,
    pub summary: Option<RemoteRepoSummary>,
    /// An SSH round-trip (connect or navigate) is in flight.
    pub busy: bool,
    pub error: Option<SharedString>,
}

impl RemoteBrowseModal {
    pub fn new() -> Self {
        Self {
            stage: RemoteBrowseStage::Connect,
            host_input: String::new(),
            host_state: None,
            port_input: String::new(),
            port_state: None,
            identity_input: String::new(),
            identity_state: None,
            host: None,
            cwd: String::new(),
            entries: Vec::new(),
            current_is_repo: false,
            summary: None,
            busy: false,
            error: None,
        }
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
    pub fn open_remote_browse_modal(&mut self, cx: &mut Context<Self>) {
        self.modal_focus = Some(cx.focus_handle());
        self.remote_browse_modal = Some(RemoteBrowseModal::new());
        cx.notify();
    }

    /// Close the remote browse modal without making any changes.
    pub fn cancel_remote_browse_modal(&mut self) {
        self.remote_browse_modal = None;
    }

    /// Validate the connection form, then connect + list the home directory on a
    /// background thread (`kagi::remote`). On success the modal flips to the
    /// directory browser; on failure it shows the ssh error.
    pub fn start_remote_connect(&mut self, cx: &mut Context<Self>) {
        let host = {
            let m = match self.remote_browse_modal.as_mut() {
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
                    m.error = Some(SharedString::from(
                        "Enter a host like user@host, host, or a ~/.ssh/config alias",
                    ));
                    return;
                }
            };
            let port_s = m.port_input.trim();
            if !port_s.is_empty() {
                match port_s.parse::<u16>() {
                    Ok(p) if p != 0 => host.port = Some(p),
                    _ => {
                        m.error = Some(SharedString::from("Port must be a number 1\u{2013}65535"));
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
            host
        };
        cx.notify();

        let task = cx.background_spawn(async move { remote_connect_blocking(&host) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                if let Some(m) = app.remote_browse_modal.as_mut() {
                    m.busy = false;
                    match result {
                        Ok(data) => {
                            m.stage = RemoteBrowseStage::Browse;
                            m.cwd = data.cwd;
                            m.entries = data.entries;
                            m.current_is_repo = data.is_repo;
                            m.summary = data.summary;
                            m.error = None;
                        }
                        Err(e) => m.error = Some(SharedString::from(e)),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Navigate the remote browser into `path` (list + repo-detect + summary on
    /// a background thread).
    pub fn remote_browse_navigate(&mut self, path: String, cx: &mut Context<Self>) {
        let host = match self
            .remote_browse_modal
            .as_ref()
            .and_then(|m| m.host.clone())
        {
            Some(h) => h,
            None => return,
        };
        if let Some(m) = self.remote_browse_modal.as_mut() {
            if m.busy {
                return;
            }
            m.busy = true;
            m.error = None;
        }
        cx.notify();

        let task = cx.background_spawn(async move { remote_browse_blocking(&host, &path) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                if let Some(m) = app.remote_browse_modal.as_mut() {
                    m.busy = false;
                    match result {
                        Ok(data) => {
                            m.cwd = data.cwd;
                            m.entries = data.entries;
                            m.current_is_repo = data.is_repo;
                            m.summary = data.summary;
                            m.error = None;
                        }
                        Err(e) => m.error = Some(SharedString::from(e)),
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

/// One labelled text input row (label above the field).
fn labeled_input(
    label: &str,
    state: Option<&Entity<gpui_component::input::InputState>>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().text_label))
                .child(SharedString::from(label.to_string())),
        )
        .children(state.map(|st| Input::new(st).small()))
}

pub(crate) fn render_remote_browse_modal(
    modal: RemoteBrowseModal,
    focus_handle: Option<FocusHandle>,
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
                this.cancel_remote_browse_modal();
                if let Some(fh) = this.root_focus.clone() {
                    window.focus(&fh);
                }
                cx.notify();
            });
        }
    };

    let mut card = div()
        .w(theme::scaled_px(560.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3();

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
                        .text_color(rgb(current_theme().text_main))
                        .text_xl()
                        .child(SharedString::from("Connect to a remote host (SSH)")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(current_theme().text_muted))
                        .child(SharedString::from(
                            "Uses your system ssh (~/.ssh/config, keys, ssh-agent, \
                             known_hosts). New or password-only hosts must be set up in a \
                             terminal first.",
                        )),
                )
                .child(labeled_input(
                    "Host  (user@host or a ~/.ssh/config alias)",
                    modal.host_state.as_ref(),
                ))
                .child(labeled_input(
                    "Port  (optional, default 22)",
                    modal.port_state.as_ref(),
                ))
                .child(labeled_input(
                    "Identity file  (optional)",
                    modal.identity_state.as_ref(),
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
                    .child(
                        Button::new("remote-connect-cancel")
                            .label("Cancel")
                            .ghost()
                            .small()
                            .on_click(cancel),
                    )
                    .child(
                        Button::new("remote-connect-go")
                            .label(if busy {
                                "Connecting\u{2026}"
                            } else {
                                "Connect"
                            })
                            .primary()
                            .small()
                            .loading(busy)
                            .disabled(busy)
                            .on_click(connect),
                    ),
            );
        }

        // ── Directory browser ───────────────────────────────────
        RemoteBrowseStage::Browse => {
            let host_label = modal.host.as_ref().map(|h| h.label()).unwrap_or_default();

            let change_host = {
                let app = app.clone();
                move |_e: &ClickEvent, _w: &mut Window, cx: &mut App| {
                    app.update(cx, |this, cx| {
                        if let Some(m) = this.remote_browse_modal.as_mut() {
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
                    .child(
                        Button::new("remote-change-host")
                            .label("Change host")
                            .ghost()
                            .xsmall()
                            .on_click(change_host),
                    ),
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
                            .child(SharedString::from("\u{25cf} Git repository")),
                    );
                if let Some(ref s) = modal.summary {
                    let branch = s.branch.clone().unwrap_or_else(|| "(detached)".to_string());
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
                            .child(SharedString::from("(no commits yet)")),
                    );
                }
                card = card.child(repo_card);
            }

            // Directory listing (scrollable). A `..` row navigates to the parent.
            // Rows are lightweight clickable list items (not Buttons) — a file
            // browser reads better as rows, matching how Zed renders its tree.
            let mut list = div()
                .id("remote-dir-list")
                .flex()
                .flex_col()
                .gap_px()
                .max_h(theme::scaled_px(280.))
                .overflow_y_scroll();

            if let Some(parent) = remote::parent_dir(&modal.cwd) {
                let nav = cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.remote_browse_navigate(parent.clone(), cx);
                    cx.notify();
                });
                list = list.child(
                    div()
                        .id("remote-dir-up")
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .text_sm()
                        .text_color(rgb(current_theme().text_main))
                        .on_click(nav)
                        .hover(|style| style.bg(rgb(current_theme().surface)))
                        .child(SharedString::from("\u{2191}  ..")),
                );
            }

            for entry in &modal.entries {
                if entry.is_dir() {
                    let target = remote::join_path(&modal.cwd, &entry.name);
                    let nav = cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                        this.remote_browse_navigate(target.clone(), cx);
                        cx.notify();
                    });
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("remote-dir-{}", entry.name)))
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_sm()
                            .text_color(rgb(current_theme().text_main))
                            .on_click(nav)
                            .hover(|style| style.bg(rgb(current_theme().surface)))
                            .child(SharedString::from(format!("\u{1f4c1}  {}/", entry.name))),
                    );
                } else {
                    list = list.child(
                        div()
                            .px_2()
                            .py_1()
                            .text_sm()
                            .text_color(rgb(current_theme().text_muted))
                            .child(SharedString::from(format!("\u{1f4c4}  {}", entry.name))),
                    );
                }
            }
            card = card.child(list);

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
                div().flex().flex_row().gap_2().justify_end().child(
                    Button::new("remote-browse-close")
                        .label("Close")
                        .ghost()
                        .small()
                        .on_click(cancel),
                ),
            );
        }
    }

    // Escape cancels (mirrors the other input modals).
    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_remote_browse_modal();
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh);
            }
            cx.stop_propagation();
            cx.notify();
        }
    });
    let focusable_card = {
        let base = div().on_key_down(esc_cancel);
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

// ──────────────────────────────────────────────────────────────
// Off-thread blocking helpers (call kagi::remote → system `ssh`)
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
    let entries = kagi::remote::list_dir(host, path).map_err(|e| e.to_string())?;
    let probe = kagi::remote::probe_repo(host, path).map_err(|e| e.to_string())?;
    let summary = if probe.is_repo {
        kagi::remote::repo_summary(host, path).map_err(|e| e.to_string())?
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
    kagi::remote::check_connection(host).map_err(|e| e.to_string())?;
    let home = kagi::remote::home_dir(host).map_err(|e| e.to_string())?;
    remote_browse_blocking(host, &home)
}
