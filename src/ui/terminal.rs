//! T-BP-007: terminal session lifecycle.
//!
//! Each [`KagiTerminalSession`] is retained by its owning `TabUiState`. The PTY
//! survives tab activation changes and is dropped with that session on close.
//!
//! # Session lifecycle
//!
//! ```text
//! TabUiState.terminal_session = None          (initial)
//!   └─ Terminal tab shown → ensure_terminal() → starts PTY + TerminalView
//! TabUiState.terminal_session = Some(KagiTerminalSession {
//!     view: Some(Entity<TerminalView>),        (running)
//!     …
//! })
//!   └─ Shell exits → exit_callback clears view to None
//!   └─ Terminal tab shown again → restarts
//! ```

mod autolock;
mod run_mode;
pub(crate) use run_mode::StartedShell;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;

use gpui::{px, AppContext, Context, Entity, Window};
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

use crate::ui::theme::theme;

/// Build the terminal [`ColorPalette`] from the active theme (W9-THEME).
pub fn build_color_palette() -> ColorPalette {
    let t = theme();
    ColorPalette::builder()
        .background(t.term_bg.0, t.term_bg.1, t.term_bg.2)
        .foreground(t.term_fg.0, t.term_fg.1, t.term_fg.2)
        .cursor(t.term_cursor.0, t.term_cursor.1, t.term_cursor.2)
        .black(t.term_black.0, t.term_black.1, t.term_black.2)
        .red(t.term_red.0, t.term_red.1, t.term_red.2)
        .green(t.term_green.0, t.term_green.1, t.term_green.2)
        .yellow(t.term_yellow.0, t.term_yellow.1, t.term_yellow.2)
        .blue(t.term_blue.0, t.term_blue.1, t.term_blue.2)
        .magenta(t.term_magenta.0, t.term_magenta.1, t.term_magenta.2)
        .cyan(t.term_cyan.0, t.term_cyan.1, t.term_cyan.2)
        .white(t.term_white.0, t.term_white.1, t.term_white.2)
        .bright_black(
            t.term_bright_black.0,
            t.term_bright_black.1,
            t.term_bright_black.2,
        )
        .bright_red(
            t.term_bright_red.0,
            t.term_bright_red.1,
            t.term_bright_red.2,
        )
        .bright_green(
            t.term_bright_green.0,
            t.term_bright_green.1,
            t.term_bright_green.2,
        )
        .bright_yellow(
            t.term_bright_yellow.0,
            t.term_bright_yellow.1,
            t.term_bright_yellow.2,
        )
        .bright_blue(
            t.term_bright_blue.0,
            t.term_bright_blue.1,
            t.term_bright_blue.2,
        )
        .bright_magenta(
            t.term_bright_magenta.0,
            t.term_bright_magenta.1,
            t.term_bright_magenta.2,
        )
        .bright_cyan(
            t.term_bright_cyan.0,
            t.term_bright_cyan.1,
            t.term_bright_cyan.2,
        )
        .bright_white(
            t.term_bright_white.0,
            t.term_bright_white.1,
            t.term_bright_white.2,
        )
        // W8-TERMSEL: selection highlight (translucent so glyphs stay readable).
        .selection(
            t.term_selection.0,
            t.term_selection.1,
            t.term_selection.2,
            t.term_selection.3,
        )
        .build()
}

/// Build the full terminal config (font + the active-theme palette).  Used both
/// Terminal font size at 1.0x zoom, in px.
pub(crate) const TERMINAL_FONT_SIZE: f32 = 13.0;

/// Terminal font size at `zoom`. Pure, so it can be tested without touching
/// the process-global zoom (which other tests read concurrently).
pub(crate) fn terminal_font_size(zoom: f32) -> f32 {
    TERMINAL_FONT_SIZE * zoom
}

/// to start a session and to live-apply a theme switch via `update_config`.
pub fn build_terminal_config() -> TerminalConfig {
    TerminalConfig {
        font_family: pick_font_family(),
        // Follows the global UI zoom (View -> Zoom In/Out), like every other
        // text surface: kagi applies zoom by scaling the window's rem size,
        // but the terminal is a PTY grid sized in real pixels, so it never saw
        // that and stayed at a fixed 13px however far the rest of the UI was
        // zoomed (user report: terminal text can't be resized). Re-applied to
        // live sessions by `KagiApp::apply_terminal_config`.
        font_size: px(terminal_font_size(crate::ui::theme::zoom())),
        cols: 80,
        rows: 24,
        scrollback: 10_000,
        line_height_multiplier: 1.0,
        padding: gpui::Edges::all(px(4.0)),
        colors: build_color_palette(),
    }
}

/// Session state for the embedded terminal.
pub struct KagiTerminalSession {
    /// Live `TerminalView` entity, or `None` if the shell has not yet started
    /// or has exited.
    pub view: Option<Entity<TerminalView>>,
    /// Error message from the most recent failed start attempt, if any.
    pub start_error: Option<String>,
    /// Repository root — used as the working directory for spawned shells.
    pub repo_path: PathBuf,
    /// Second handle to the PTY writer, used by the cmd-v paste path
    /// (gpui-terminal 0.1.0 has no built-in paste; we write directly).
    pub paste_writer: Option<SharedWriter>,
    /// #772: the shell process this session owns — PID, spawn generation and
    /// the wait result delivered off the render path. `None` before the first
    /// spawn. Survives `view = None` so an exit can still be attributed.
    pub shell: Option<ShellProcess>,
    /// #772: how many shells this session has spawned; the next spawn is
    /// `spawns + 1`. Stale wait deliveries compare against it.
    pub spawns: u64,
    /// The lock this session actually acquired (not an unconfirmed offer).
    /// The release plan still reads Git; this records only provenance.
    pub auto_lock: Option<AutoLockOffer>,
    /// A proven exit waiting for this owner's modal slot. Rendering schedules
    /// one deferred retry when the slot becomes empty (including button exits).
    pub release_offer_pending: bool,
}

/// Frozen owner of one confirmed terminal lock. A tab/path reused after the
/// offer cannot turn a different repository's lock into this one's release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoLockOffer {
    pub owner: crate::app::SessionId,
    pub generation: u64,
    pub path: PathBuf,
    pub target: kagi_domain::worktree_autolock::AutoUnlockTarget,
}

/// The shell child a [`KagiTerminalSession`] spawned (#772 欠落 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellProcess {
    /// The shell's PID, when the platform reported one.
    pub pid: Option<u32>,
    /// Spawn generation within the owning session (1 for the first shell).
    pub generation: u64,
    /// Set once the background `wait` on the child returns. `None` = still
    /// running as far as Kagi has observed.
    pub exit: Option<ShellExit>,
}

/// What the background `wait` on the shell child reported (#772 欠落 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellExit {
    /// The child exited with this code.
    Exited { code: u32 },
    /// `wait` itself failed: the child's fate is unknown. Held, not treated
    /// as an exit (ADR-0208 決定 3).
    Unknown(String),
}

impl ShellProcess {
    /// The shell's current directory as the OS reports it, or why it could
    /// not be read. Never a substitute value (contract A / C).
    pub fn observe_cwd(&self) -> Result<PathBuf, kagi_git::proc::CwdProbe> {
        match self.pid {
            Some(pid) => kagi_git::proc::cwd_of_pid(pid),
            None => Err(kagi_git::proc::CwdProbe::Unknown(
                "the platform reported no PID for the shell".to_string(),
            )),
        }
    }
}

/// A future that resolves when a dedicated thread's `child.wait()` returns.
///
/// The thread stores the result and wakes whichever task last polled; a
/// result that lands before the first poll is picked up on that poll. No
/// executor dependency: the shell's lifetime is not a pool thread's to hold.
pub(crate) struct ShellWait(Arc<Mutex<ShellWaitSlot>>);

#[derive(Default)]
struct ShellWaitSlot {
    exit: Option<ShellExit>,
    waker: Option<std::task::Waker>,
}

impl ShellWait {
    fn spawn(mut child: Box<dyn portable_pty::Child + Send + Sync>) -> Self {
        let slot = Arc::new(Mutex::new(ShellWaitSlot::default()));
        let writer = slot.clone();
        std::thread::Builder::new()
            .name("kagi-shell-wait".into())
            .spawn(move || {
                let exit = match child.wait() {
                    Ok(status) => ShellExit::Exited {
                        code: status.exit_code(),
                    },
                    Err(e) => ShellExit::Unknown(e.to_string()),
                };
                let waker = {
                    let mut slot = writer.lock().unwrap_or_else(|p| p.into_inner());
                    slot.exit = Some(exit);
                    slot.waker.take()
                };
                if let Some(waker) = waker {
                    waker.wake();
                }
            })
            .expect("spawn shell wait thread");
        Self(slot)
    }
}

impl Future for ShellWait {
    type Output = ShellExit;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        match slot.exit.take() {
            Some(exit) => Poll::Ready(exit),
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Cloneable wrapper around the single PTY writer.
///
/// `portable_pty::take_writer` can only be called once, but both the
/// `TerminalView` (keystrokes) and the cmd-v paste path need to write.
/// All writes go through one mutex-guarded handle.
#[derive(Clone)]
pub struct SharedWriter(Arc<Mutex<Box<dyn std::io::Write + Send>>>);

impl SharedWriter {
    fn new(inner: Box<dyn std::io::Write + Send>) -> Self {
        SharedWriter(Arc::new(Mutex::new(inner)))
    }

    /// Write the given text to the PTY (used by paste).
    pub fn paste_text(&self, text: &str) {
        if let Ok(mut w) = self.0.lock() {
            let _ = w.write_all(text.as_bytes());
            let _ = w.flush();
        }
    }
}

impl std::io::Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.0.lock() {
            Ok(mut w) => w.write(buf),
            Err(_) => Err(std::io::Error::other("poisoned")),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self.0.lock() {
            Ok(mut w) => w.flush(),
            Err(_) => Ok(()),
        }
    }
}

impl KagiTerminalSession {
    /// Create a new, not-yet-started session.
    pub fn new(repo_path: PathBuf) -> Self {
        KagiTerminalSession {
            view: None,
            start_error: None,
            repo_path,
            paste_writer: None,
            shell: None,
            spawns: 0,
            auto_lock: None,
            release_offer_pending: false,
        }
    }

    /// The shell's cwd, when this session has a shell whose exit has not been
    /// observed. See [`ShellProcess::observe_cwd`].
    pub fn observe_cwd(&self) -> Option<Result<PathBuf, kagi_git::proc::CwdProbe>> {
        self.shell
            .as_ref()
            .filter(|shell| shell.exit.is_none())
            .map(ShellProcess::observe_cwd)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// PTY + TerminalView construction
// ──────────────────────────────────────────────────────────────────────────────

/// What [`build_terminal_view`] returns on success: the view, the PTY master
/// handle (kept alive for resize callbacks), the writer, and the shell child
/// the session now owns (#772). Named for readability
/// (clippy::type_complexity).
pub struct TerminalBuild {
    pub view: Entity<TerminalView>,
    pub master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    pub paste_writer: SharedWriter,
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    /// #852: the shell started without `KAGI_*` because the port range had no
    /// free block — the caller tells the user why.
    pub ports_exhausted: Option<PortsExhausted>,
}

/// The worktree, range and block size a terminal found no free port block
/// for (#852).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortsExhausted {
    pub worktree: std::path::PathBuf,
    pub range: (u16, u16),
    pub per: u16,
}

/// Attempt to open a PTY, spawn `shell`, and create an `Entity<TerminalView>`.
///
/// On success returns the entity and the PTY master handle (kept alive for
/// resize callbacks).
///
/// On failure returns an error string; the caller should record this as a
/// Failed operation in the Operation Log.
pub fn build_terminal_view(
    shell: &str,
    repo_path: &std::path::Path,
    owner: crate::app::SessionId,
    cx: &mut Context<crate::ui::KagiApp>,
) -> Result<TerminalBuild, String> {
    let settings = crate::ui::settings::Settings::load();
    let (start, end) = settings.worktree_port_range();
    let environment = kagi_git::worktree_ports::terminal_env(
        repo_path,
        kagi_domain::worktree_ports::PortRange { start, end },
        settings.worktree_ports_per_worktree(),
        settings.worktree_run_mode(),
    )
    .map_err(|error| format!("terminal environment: {error}"))?;
    // #852: no free block is no reason to withhold the shell itself.
    let (vars, ports_exhausted) = match environment {
        kagi_git::worktree_ports::TerminalEnv::Ports(ports) => (ports.vars, None),
        kagi_git::worktree_ports::TerminalEnv::Exhausted {
            worktree,
            range,
            per,
        } => {
            klog!(
                "terminal: port block exhausted {} (range {}-{}, per {})",
                worktree.display(),
                range.start,
                range.end,
                per
            );
            let exhausted = PortsExhausted {
                worktree,
                range: (range.start, range.end),
                per,
            };
            (Vec::new(), Some(exhausted))
        }
    };

    // Open the PTY pair.
    let pty_system = NativePtySystem::default();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {}", e))?;

    // Build the shell command.
    let mut cmd = CommandBuilder::new(shell);
    // Launch as a LOGIN + INTERACTIVE shell so the user's rc files are sourced
    // (~/.zprofile + ~/.zshrc for zsh, ~/.bash_profile + ~/.bashrc for bash) —
    // otherwise a GUI-launched app gets a bare PATH and tools like `python`,
    // Homebrew, pyenv, etc. aren't found. Skipped on Windows (cmd.exe has no
    // such flags and inherits the environment directly).
    #[cfg(not(windows))]
    {
        cmd.arg("-l");
        cmd.arg("-i");
    }
    cmd.cwd(repo_path);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    for (key, value) in vars {
        cmd.env(key, value);
    }

    // Spawn the shell process before consuming the master (slave must still
    // be open for the child to inherit its fd). #772: the returned child is
    // handed to the session, which owns its PID and waits on it.
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn '{}': {}", shell, e))?;

    // Take the write/read ends of the master.
    let writer = SharedWriter::new(
        pair.master
            .take_writer()
            .map_err(|e| format!("take_writer: {}", e))?,
    );
    let paste_writer = writer.clone();

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("try_clone_reader: {}", e))?;

    // Wrap the master in an Arc<Mutex> so it can be shared with the resize
    // callback (which runs on a different thread).
    let master_arc: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>> =
        Arc::new(Mutex::new(pair.master));

    // Slave fd is inherited by the child process; dropping our handle here
    // is correct (the child holds its copy via fork/exec).
    drop(pair.slave);

    // Config (font + active-theme palette, W9-THEME / ADR-0036).
    let config = build_terminal_config();

    let resize_master = master_arc.clone();

    // The callback is stamped with the creating session. It may finish while
    // another tab is active, and a detached owner receives no write.
    let weak_app = cx.weak_entity();

    // Create the TerminalView entity.  `cx.new` is called on `Context<KagiApp>`
    // and produces `Entity<TerminalView>`.
    let view_entity = cx.new(|view_cx| {
        TerminalView::new(writer, reader, config, view_cx)
            .with_resize_callback(move |cols, rows| {
                if let Ok(master) = resize_master.lock() {
                    let _ = master.resize(PtySize {
                        rows: rows as u16,
                        cols: cols as u16,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }
            })
            .with_exit_callback(move |_window, cx| {
                klog!("terminal: shell exited");
                let _ = weak_app.update(cx, |app, cx| {
                    if let Some(session) = app
                        .ui
                        .get_mut(&owner)
                        .and_then(|ui| ui.terminal_session.as_mut())
                    {
                        session.view = None;
                    }
                    cx.notify();
                });
            })
    });

    Ok(TerminalBuild {
        view: view_entity,
        master: master_arc,
        paste_writer,
        child,
        ports_exhausted,
    })
}

/// Pick the terminal font family: prefer an installed Nerd Font (for terminal
/// icon glyphs), otherwise fall back to the **bundled** JetBrains Mono
/// (`super::MONO_FONT`, loaded via `add_fonts`), which is guaranteed present on
/// every OS. The old "Menlo" fallback was macOS-only and rendered broken on
/// Linux (user-reported).
///
/// Order: RobotoMono Nerd Font → JetBrainsMono Nerd Font → Hack Nerd Font →
/// bundled JetBrains Mono.
pub(crate) fn pick_font_family() -> String {
    // Memoized: the uncached probe below `read_dir`s up to 6 font directories
    // per candidate family, and callers are render-path (the conflict editor
    // calls it ~6x per frame; the diff list once per pane). Installed fonts
    // don't change under a running process, so resolving once is correct as
    // well as cheap.
    static CACHED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CACHED.get_or_init(pick_font_family_uncached).clone()
}

fn pick_font_family_uncached() -> String {
    // Windows ships Consolas (a good monospace); the Nerd Font dirs below are
    // Unix-only, so resolve directly.
    #[cfg(target_os = "windows")]
    {
        "Consolas".to_string()
    }

    #[cfg(not(target_os = "windows"))]
    {
        const CANDIDATES: &[(&str, &str)] = &[
            ("RobotoMonoNerdFont", "RobotoMono Nerd Font"),
            ("JetBrainsMonoNerdFont", "JetBrainsMono Nerd Font"),
            ("HackNerdFont", "Hack Nerd Font"),
        ];

        // Both macOS and Linux user/system font directories (top-level scan).
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            let h = PathBuf::from(home);
            dirs.push(h.join("Library/Fonts")); // macOS (user)
            dirs.push(h.join(".local/share/fonts")); // Linux (user)
            dirs.push(h.join(".fonts")); // Linux (user, legacy)
        }
        dirs.push(PathBuf::from("/Library/Fonts")); // macOS (system)
        dirs.push(PathBuf::from("/usr/share/fonts")); // Linux (system)
        dirs.push(PathBuf::from("/usr/local/share/fonts")); // Linux (system)

        for (file_prefix, family) in CANDIDATES {
            for dir in &dirs {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        if entry.file_name().to_string_lossy().starts_with(file_prefix) {
                            return (*family).to_string();
                        }
                    }
                }
            }
        }
        super::MONO_FONT.to_string()
    }
}

/// Resolve the user's preferred shell.
///
/// Unix: `$SHELL`, falling back to `/bin/zsh`.
/// Windows: `%ComSpec%` (the command processor), falling back to `cmd.exe`.
pub fn resolve_shell() -> String {
    #[cfg(feature = "gui-e2e")]
    if let Some(shell) = E2E_SHELL.with(|slot| slot.borrow().clone()) {
        return shell;
    }
    #[cfg(windows)]
    {
        std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string())
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static E2E_SHELL: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

#[cfg(feature = "gui-e2e")]
impl crate::ui::KagiApp {
    /// Spawn `shell` instead of `$SHELL` for terminals started from now on
    /// (`None` restores `$SHELL`). Tier A only (#855): lets a scenario observe
    /// the environment the real PTY spawn hands its shell, which a payload
    /// unit test cannot.
    pub fn set_terminal_shell_for_e2e(shell: Option<String>) {
        E2E_SHELL.with(|slot| *slot.borrow_mut() = shell);
    }
}

/// Ensure the terminal session is started.
///
/// * If the session already has a live view, focuses it and returns `Ok(false)`.
/// * Otherwise spawns a new PTY + shell, wires exit/resize callbacks, records
///   success or failure, and returns `Ok(true)` / `Err(msg)`.
///
/// The `record_failure` callback is invoked with the error message when the
/// shell fails to start; the caller should call `record_op` on `KagiApp`.
/// `ports_exhausted` is invoked when the shell did start, but without `KAGI_*`
/// because the port range had no free block (#852).
pub fn ensure_terminal(
    session: &mut KagiTerminalSession,
    owner: crate::app::SessionId,
    window: &mut Window,
    cx: &mut Context<crate::ui::KagiApp>,
    record_failure: impl FnOnce(String),
    ports_exhausted: impl FnOnce(PortsExhausted),
) -> bool {
    if session.view.is_some() {
        // Already running — just re-focus.
        if let Some(ref view) = session.view {
            let fh = view.read(cx).focus_handle().clone();
            window.focus(&fh, cx);
        }
        return false;
    }

    let shell = resolve_shell();
    klog!("terminal: starting shell={}", shell);

    match build_terminal_view(&shell, &session.repo_path, owner, cx) {
        Ok(TerminalBuild {
            view: view_entity,
            master: _master_arc,
            paste_writer,
            child,
            ports_exhausted: exhausted,
        }) => {
            // Focus the new terminal.
            let fh = view_entity.read(cx).focus_handle().clone();
            window.focus(&fh, cx);

            session.view = Some(view_entity);
            session.paste_writer = Some(paste_writer);
            session.start_error = None;
            // #772 欠落 1 / 2: own the child, and observe its exit off the
            // render path. The PTY EOF callback above still clears the view;
            // this is the process-level evidence, delivered even while the
            // terminal is hidden or another tab is active, and attributed to
            // the owner + spawn generation it belongs to.
            session.spawns += 1;
            let generation = session.spawns;
            session.shell = Some(ShellProcess {
                pid: child.process_id(),
                generation,
                exit: None,
            });
            // `wait` blocks for the shell's whole life, so it gets its own OS
            // thread rather than a slot on gpui's background pool (which the
            // test dispatcher runs inline — a pool wait would park it forever).
            let exited = ShellWait::spawn(child);
            cx.spawn(async move |this, acx| {
                let exit = exited.await;
                let _ = this.update(acx, |app, cx| {
                    app.on_shell_wait(owner, generation, exit, cx);
                });
            })
            .detach();
            klog!("terminal: started shell={}", shell);
            if let Some(exhausted) = exhausted {
                ports_exhausted(exhausted);
            }
            true
        }
        Err(e) => {
            klog!("terminal: start failed: {}", e);
            session.start_error = Some(e.clone());
            record_failure(e);
            false
        }
    }
}

#[cfg(test)]
mod font_size_tests {
    use super::*;

    /// Regression: the terminal font was a hardcoded `px(13.0)` and ignored
    /// the UI zoom entirely, so View -> Zoom In/Out resized every surface
    /// except the terminal (user report).
    ///
    /// Deliberately exercises the pure `terminal_font_size` rather than
    /// `set_zoom` + `build_terminal_config`: zoom is process-global state and
    /// `graph_view`'s own zoom test reads it from a parallel test thread, so
    /// mutating it here made that test fail intermittently.
    #[test]
    fn terminal_font_follows_ui_zoom() {
        let base = terminal_font_size(1.0);
        assert!(
            (base - TERMINAL_FONT_SIZE).abs() < f32::EPSILON,
            "1.0x must be the base size, got {base}"
        );
        assert!(
            terminal_font_size(1.5) > base,
            "zooming in must enlarge the terminal font"
        );
        assert!(
            terminal_font_size(0.8) < base,
            "zooming out must shrink the terminal font"
        );
    }
}
