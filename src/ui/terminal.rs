//! T-BP-007: Terminal session manager.
//!
//! Wraps gpui-terminal + portable-pty into a single lazy-initialised session
//! stored on `KagiApp`.  The PTY is only spawned when the Terminal tab is
//! first shown, and it is preserved across tab switches until the app exits
//! (or the shell process exits, in which case it is restarted on next show).
//!
//! # Session lifecycle
//!
//! ```text
//! KagiApp.terminal_session = None          (initial)
//!   └─ Terminal tab shown → ensure_terminal() → starts PTY + TerminalView
//! KagiApp.terminal_session = Some(KagiTerminalSession {
//!     view: Some(Entity<TerminalView>),     (running)
//!     …
//! })
//!   └─ Shell exits → exit_callback clears view to None
//!   └─ Terminal tab shown again → restarts
//! ```

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui::{AppContext, Context, Entity, Window, px};
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

/// Session state for the embedded terminal.
pub struct KagiTerminalSession {
    /// Live `TerminalView` entity, or `None` if the shell has not yet started
    /// or has exited.
    pub view: Option<Entity<TerminalView>>,
    /// Error message from the most recent failed start attempt, if any.
    pub start_error: Option<String>,
    /// Repository root — used as the working directory for spawned shells.
    pub repo_path: PathBuf,
}

impl KagiTerminalSession {
    /// Create a new, not-yet-started session.
    pub fn new(repo_path: PathBuf) -> Self {
        KagiTerminalSession {
            view: None,
            start_error: None,
            repo_path,
        }
    }

    /// Whether the session currently has a live terminal view.
    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.view.is_some()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// PTY + TerminalView construction
// ──────────────────────────────────────────────────────────────────────────────

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
    cx: &mut Context<crate::ui::KagiApp>,
) -> Result<
    (
        Entity<TerminalView>,
        Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    ),
    String,
> {
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
    cmd.cwd(repo_path);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    // Spawn the shell process before consuming the master (slave must still
    // be open for the child to inherit its fd).
    pair.slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn '{}': {}", shell, e))?;

    // Take the write/read ends of the master.
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer: {}", e))?;

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

    // Build the Catppuccin Mocha colour palette to match the rest of the UI.
    let colors = ColorPalette::builder()
        .background(0x1e, 0x1e, 0x2e)
        .foreground(0xcd, 0xd6, 0xf4)
        .cursor(0xf5, 0xc2, 0xe7)
        .black(0x45, 0x47, 0x5a)
        .red(0xf3, 0x8b, 0xa8)
        .green(0xa6, 0xe3, 0xa1)
        .yellow(0xf9, 0xe2, 0xaf)
        .blue(0x89, 0xb4, 0xfa)
        .magenta(0xcb, 0xa6, 0xf7)
        .cyan(0x89, 0xdc, 0xeb)
        .white(0xba, 0xc2, 0xde)
        .bright_black(0x58, 0x5b, 0x70)
        .bright_red(0xf3, 0x8b, 0xa8)
        .bright_green(0xa6, 0xe3, 0xa1)
        .bright_yellow(0xf9, 0xe2, 0xaf)
        .bright_blue(0x89, 0xb4, 0xfa)
        .bright_magenta(0xcb, 0xa6, 0xf7)
        .bright_cyan(0x89, 0xdc, 0xeb)
        .bright_white(0xcd, 0xd6, 0xf4)
        .build();

    let config = TerminalConfig {
        // Nerd Font (user request).  JetBrainsMono Nerd Font is installed on
        // the dev machine; fall back to plain JetBrains Mono, then Menlo.
        // TODO(later): make this a user setting.
        font_family: pick_font_family(),
        font_size: px(13.0),
        cols: 80,
        rows: 24,
        scrollback: 10_000,
        line_height_multiplier: 1.0,
        padding: gpui::Edges::all(px(4.0)),
        colors,
    };

    let resize_master = master_arc.clone();

    // Weak handle back to KagiApp so the exit callback can clear the dead
    // session (next tab activation restarts the shell).
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
                eprintln!("[kagi] terminal: shell exited");
                let _ = weak_app.update(cx, |app, cx| {
                    if let Some(session) = app.terminal_session.as_mut() {
                        session.view = None;
                    }
                    cx.notify();
                });
            })
    });

    Ok((view_entity, master_arc))
}

/// Pick the terminal font family: prefer Nerd Fonts (user request),
/// checking the macOS font directories for installed families.
///
/// Order: RobotoMono Nerd Font → JetBrainsMono Nerd Font → Hack Nerd Font →
/// Menlo (always present on macOS).
fn pick_font_family() -> String {
    const CANDIDATES: &[(&str, &str)] = &[
        ("RobotoMonoNerdFont", "RobotoMono Nerd Font"),
        ("JetBrainsMonoNerdFont", "JetBrainsMono Nerd Font"),
        ("HackNerdFont", "Hack Nerd Font"),
    ];

    let mut dirs: Vec<PathBuf> = vec![PathBuf::from("/Library/Fonts")];
    if let Ok(home) = std::env::var("HOME") {
        dirs.insert(0, PathBuf::from(home).join("Library/Fonts"));
    }

    for (file_prefix, family) in CANDIDATES {
        for dir in &dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(file_prefix)
                    {
                        return (*family).to_string();
                    }
                }
            }
        }
    }
    "Menlo".to_string()
}

/// Resolve the user's preferred shell.
///
/// Returns `$SHELL` or falls back to `/bin/zsh`.
pub fn resolve_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
}

/// Ensure the terminal session is started.
///
/// * If the session already has a live view, focuses it and returns `Ok(false)`.
/// * Otherwise spawns a new PTY + shell, wires exit/resize callbacks, records
///   success or failure, and returns `Ok(true)` / `Err(msg)`.
///
/// The `record_failure` callback is invoked with the error message when the
/// shell fails to start; the caller should call `record_op` on `KagiApp`.
pub fn ensure_terminal(
    session: &mut KagiTerminalSession,
    window: &mut Window,
    cx: &mut Context<crate::ui::KagiApp>,
    record_failure: impl FnOnce(String),
) -> bool {
    if session.view.is_some() {
        // Already running — just re-focus.
        if let Some(ref view) = session.view {
            let fh = view.read(cx).focus_handle().clone();
            window.focus(&fh);
        }
        return false;
    }

    let shell = resolve_shell();
    eprintln!("[kagi] terminal: starting shell={}", shell);

    match build_terminal_view(&shell, &session.repo_path, cx) {
        Ok((view_entity, _master_arc)) => {
            // Focus the new terminal.
            let fh = view_entity.read(cx).focus_handle().clone();
            window.focus(&fh);

            session.view = Some(view_entity);
            session.start_error = None;
            eprintln!("[kagi] terminal: started shell={}", shell);
            true
        }
        Err(e) => {
            eprintln!("[kagi] terminal: start failed: {}", e);
            session.start_error = Some(e.clone());
            record_failure(e);
            false
        }
    }
}
