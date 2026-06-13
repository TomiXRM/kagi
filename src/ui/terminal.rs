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
        .bright_black(t.term_bright_black.0, t.term_bright_black.1, t.term_bright_black.2)
        .bright_red(t.term_bright_red.0, t.term_bright_red.1, t.term_bright_red.2)
        .bright_green(t.term_bright_green.0, t.term_bright_green.1, t.term_bright_green.2)
        .bright_yellow(t.term_bright_yellow.0, t.term_bright_yellow.1, t.term_bright_yellow.2)
        .bright_blue(t.term_bright_blue.0, t.term_bright_blue.1, t.term_bright_blue.2)
        .bright_magenta(t.term_bright_magenta.0, t.term_bright_magenta.1, t.term_bright_magenta.2)
        .bright_cyan(t.term_bright_cyan.0, t.term_bright_cyan.1, t.term_bright_cyan.2)
        .bright_white(t.term_bright_white.0, t.term_bright_white.1, t.term_bright_white.2)
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
/// to start a session and to live-apply a theme switch via `update_config`.
pub fn build_terminal_config() -> TerminalConfig {
    TerminalConfig {
        font_family: pick_font_family(),
        font_size: px(13.0),
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
            Err(_) => Err(std::io::Error::new(std::io::ErrorKind::Other, "poisoned")),
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
        SharedWriter,
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

    // Weak handle back to KagiApp so the exit callback can clear the dead
    // session (next tab activation restarts the shell).
    let weak_app = cx.weak_entity();
    // W4-TABS: capture this session's repo path so the exit callback clears
    // the correct entry in the per-repo sessions map.
    let exit_repo_path = repo_path.to_path_buf();

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
                    if let Some(session) = app.terminal_sessions.get_mut(&exit_repo_path) {
                        session.view = None;
                    }
                    cx.notify();
                });
            })
    });

    Ok((view_entity, master_arc, paste_writer))
}

/// Pick the terminal font family: prefer Nerd Fonts (user request),
/// checking the macOS font directories for installed families.
///
/// Order: RobotoMono Nerd Font → JetBrainsMono Nerd Font → Hack Nerd Font →
/// Menlo (always present on macOS).
pub(crate) fn pick_font_family() -> String {
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
        Ok((view_entity, _master_arc, paste_writer)) => {
            // Focus the new terminal.
            let fh = view_entity.read(cx).focus_handle().clone();
            window.focus(&fh);

            session.view = Some(view_entity);
            session.paste_writer = Some(paste_writer);
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
