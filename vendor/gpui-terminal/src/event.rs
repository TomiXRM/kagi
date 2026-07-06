//! Event handling for the terminal emulator.
//!
//! This module bridges alacritty's event system with GPUI by providing
//! [`GpuiEventProxy`], which implements alacritty's [`EventListener`] trait
//! and forwards relevant events through a channel.
//!
//! # Event Flow
//!
//! ```text
//! alacritty Term → GpuiEventProxy → mpsc channel → TerminalView
//!                        │
//!                        └─ Translates Event enum to TerminalEvent
//! ```
//!
//! # Supported Events
//!
//! | Alacritty Event | TerminalEvent | Description |
//! |-----------------|---------------|-------------|
//! | `Event::Wakeup` | `Wakeup` | Terminal has new content |
//! | `Event::Bell` | `Bell` | BEL character received |
//! | `Event::Title(_)` | `Title(String)` | Title escape sequence (OSC 0/2) |
//! | `Event::ClipboardStore(_, _)` | `ClipboardStore(String)` | Copy request (OSC 52) |
//! | `Event::ClipboardLoad(_, _)` | `ClipboardLoad` | Paste request |
//! | `Event::Exit` | `Exit` | Terminal exited |
//! | `Event::ChildExit(_)` | `Exit` | Child process exited |
//! | `Event::ResetTitle` | `Title("")` | Reset to empty title |
//!
//! `Event::PtyWrite` is **not** forwarded through the `TerminalEvent` channel
//! (that channel is only drained on render, which is too slow — see below).
//! Instead its bytes are pushed onto [`GpuiEventProxy::pty_responses_handle`],
//! a small queue the owner drains right after each `process_bytes` call.
//!
//! `Event::MouseCursorDirty` and `Event::CursorBlinkingChange` are ignored as
//! they're handled internally or not needed for GPUI integration.
//!
//! # `PtyWrite`: terminal-query responses (T-TERM-INTERACT-001)
//!
//! alacritty emits `Event::PtyWrite(String)` when the terminal itself must
//! answer a query from the running program — DSR cursor-position reports,
//! DA1/DA2 device attributes, bracketed-paste acknowledgement, keyboard-mode
//! reports, text-area-size reports, etc. These bytes are a reply that must go
//! back to the PTY, not to the GPUI event loop. Programs that query-and-wait
//! at startup (zellij is the reported case) hang forever if this is dropped
//! on the floor, which is what this module used to do.
//!
//! Because `send_event` fires synchronously from inside alacritty's VTE
//! handler dispatch (itself called while [`TerminalState`](crate::terminal::TerminalState)
//! holds its `Term` mutex locked), we do not write to the PTY writer here —
//! that would nest the writer lock inside the term lock on every query
//! response. Instead we buffer the bytes in a plain `Vec<u8>` behind a
//! second, independent lock; `TerminalState::process_bytes` drains that queue
//! immediately after releasing the term lock, and the caller (the terminal
//! view's PTY reader task) writes the drained bytes to the PTY right away —
//! before the next render, not gated by one.
//!
//! # Example
//!
//! ```
//! use std::sync::mpsc::channel;
//! use gpui_terminal::event::{GpuiEventProxy, TerminalEvent};
//!
//! let (tx, rx) = channel();
//! let proxy = GpuiEventProxy::new(tx);
//!
//! // The proxy is passed to alacritty's Term and will forward events
//! // Events can be received on the other end of the channel
//! ```
//!
//! [`EventListener`]: alacritty_terminal::event::EventListener

use alacritty_terminal::event::{Event, EventListener};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::mpsc::Sender;

/// Events emitted by the terminal that the GPUI application cares about.
///
/// This enum represents a subset of alacritty's events that are relevant
/// for the GPUI terminal emulator implementation.
#[derive(Debug, Clone)]
pub enum TerminalEvent {
    /// The terminal has new content to display and needs a redraw.
    Wakeup,

    /// The terminal bell was triggered (visual or audible alert).
    Bell,

    /// The terminal title has changed.
    Title(String),

    /// The terminal wants to store data to the clipboard.
    ClipboardStore(String),

    /// The terminal wants to load data from the clipboard.
    ClipboardLoad,

    /// The terminal process has exited.
    Exit,
}

/// An event proxy that implements alacritty's EventListener trait.
///
/// This struct forwards relevant terminal events to a channel that can be
/// consumed by the GPUI application on the main thread.
pub struct GpuiEventProxy {
    /// Channel sender for forwarding events to the GPUI application.
    tx: Sender<TerminalEvent>,

    /// kagi (T-TERM-INTERACT-001): queue of pending `Event::PtyWrite` bytes
    /// (terminal-query responses). Shared with whoever holds a handle from
    /// [`pty_responses_handle`](Self::pty_responses_handle) — normally
    /// [`TerminalState`](crate::terminal::TerminalState), which drains it in
    /// `process_bytes` right after each batch of PTY bytes is parsed.
    pty_responses: Arc<Mutex<Vec<u8>>>,
}

impl GpuiEventProxy {
    /// Creates a new event proxy with the given channel sender.
    ///
    /// # Arguments
    ///
    /// * `tx` - The channel sender to forward events through
    ///
    /// # Returns
    ///
    /// A new GpuiEventProxy instance
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::mpsc::channel;
    /// use gpui_terminal::event::GpuiEventProxy;
    ///
    /// let (tx, rx) = channel();
    /// let proxy = GpuiEventProxy::new(tx);
    /// ```
    pub fn new(tx: Sender<TerminalEvent>) -> Self {
        Self {
            tx,
            pty_responses: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get a shared handle to the pending `PtyWrite` response queue.
    ///
    /// Call this *before* handing the proxy to `Term::new` (which consumes
    /// it by value), so the caller keeps a clone of the queue to drain later.
    /// See the module docs for why this is a queue rather than a direct
    /// writer handle.
    pub fn pty_responses_handle(&self) -> Arc<Mutex<Vec<u8>>> {
        Arc::clone(&self.pty_responses)
    }

    /// Sends a terminal event through the channel.
    ///
    /// If the channel is disconnected, this method will silently drop the event.
    /// This can happen if the GPUI application has been shut down.
    fn send(&self, event: TerminalEvent) {
        // Ignore send errors - they just mean the receiver has been dropped
        let _ = self.tx.send(event);
    }
}

impl EventListener for GpuiEventProxy {
    /// Handles events from the alacritty terminal.
    ///
    /// This method is called by alacritty when terminal events occur.
    /// It translates alacritty's Event enum to our TerminalEvent enum
    /// and forwards relevant events through the channel.
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => {
                self.send(TerminalEvent::Wakeup);
            }
            Event::Bell => {
                self.send(TerminalEvent::Bell);
            }
            Event::Title(title) => {
                self.send(TerminalEvent::Title(title));
            }
            Event::ClipboardStore(_clipboard_type, data) => {
                // For simplicity, we ignore the clipboard type and just store the data
                self.send(TerminalEvent::ClipboardStore(data));
            }
            Event::ClipboardLoad(_clipboard_type, _format) => {
                // For simplicity, we ignore the clipboard type and format
                self.send(TerminalEvent::ClipboardLoad);
            }
            Event::Exit => {
                self.send(TerminalEvent::Exit);
            }
            // Ignore events we don't care about
            Event::MouseCursorDirty => {}
            Event::PtyWrite(data) => {
                // kagi (T-TERM-INTERACT-001): queue the response bytes for
                // the owner to write back to the PTY. See module docs for
                // why this doesn't write directly (term-lock/writer-lock
                // ordering).
                self.pty_responses.lock().extend_from_slice(data.as_bytes());
            }
            Event::ColorRequest(ref _index, ref _format) => {
                // Color requests are not commonly used
            }
            Event::TextAreaSizeRequest(ref _format) => {
                // Text area size requests are handled internally
            }
            Event::CursorBlinkingChange => {
                // Cursor blinking changes could be handled if needed
            }
            Event::ResetTitle => {
                // Reset title to default - we can treat this as an empty title
                self.send(TerminalEvent::Title(String::new()));
            }
            Event::ChildExit(_exit_code) => {
                // Child process exited - treat this as a terminal exit
                self.send(TerminalEvent::Exit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn test_event_proxy_creation() {
        let (tx, _rx) = channel();
        let _proxy = GpuiEventProxy::new(tx);
    }

    #[test]
    fn test_wakeup_event() {
        let (tx, rx) = channel();
        let proxy = GpuiEventProxy::new(tx);

        proxy.send_event(Event::Wakeup);

        let event = rx.recv().unwrap();
        assert!(matches!(event, TerminalEvent::Wakeup));
    }

    #[test]
    fn test_bell_event() {
        let (tx, rx) = channel();
        let proxy = GpuiEventProxy::new(tx);

        proxy.send_event(Event::Bell);

        let event = rx.recv().unwrap();
        assert!(matches!(event, TerminalEvent::Bell));
    }

    #[test]
    fn test_title_event() {
        let (tx, rx) = channel();
        let proxy = GpuiEventProxy::new(tx);

        proxy.send_event(Event::Title("Test Title".to_string()));

        let event = rx.recv().unwrap();
        match event {
            TerminalEvent::Title(title) => assert_eq!(title, "Test Title"),
            _ => panic!("Expected Title event"),
        }
    }

    #[test]
    fn test_clipboard_store_event() {
        use alacritty_terminal::term::ClipboardType;

        let (tx, rx) = channel();
        let proxy = GpuiEventProxy::new(tx);

        proxy.send_event(Event::ClipboardStore(
            ClipboardType::Clipboard,
            "clipboard data".to_string(),
        ));

        let event = rx.recv().unwrap();
        match event {
            TerminalEvent::ClipboardStore(data) => assert_eq!(data, "clipboard data"),
            _ => panic!("Expected ClipboardStore event"),
        }
    }

    #[test]
    fn test_clipboard_load_event() {
        use alacritty_terminal::term::ClipboardType;
        use std::sync::Arc;

        let (tx, rx) = channel();
        let proxy = GpuiEventProxy::new(tx);

        // ClipboardLoad requires a callback function
        let callback = Arc::new(|s: &str| s.to_string());
        proxy.send_event(Event::ClipboardLoad(ClipboardType::Clipboard, callback));

        let event = rx.recv().unwrap();
        assert!(matches!(event, TerminalEvent::ClipboardLoad));
    }

    #[test]
    fn test_exit_event() {
        let (tx, rx) = channel();
        let proxy = GpuiEventProxy::new(tx);

        proxy.send_event(Event::Exit);

        let event = rx.recv().unwrap();
        assert!(matches!(event, TerminalEvent::Exit));
    }

    #[test]
    fn test_reset_title_event() {
        let (tx, rx) = channel();
        let proxy = GpuiEventProxy::new(tx);

        proxy.send_event(Event::ResetTitle);

        let event = rx.recv().unwrap();
        match event {
            TerminalEvent::Title(title) => assert!(title.is_empty()),
            _ => panic!("Expected Title event"),
        }
    }

    #[test]
    fn test_ignored_events() {
        let (tx, rx) = channel();
        let proxy = GpuiEventProxy::new(tx);

        // These events should be ignored and not sent through the channel
        proxy.send_event(Event::MouseCursorDirty);
        proxy.send_event(Event::CursorBlinkingChange);

        // The channel should be empty
        assert!(rx.try_recv().is_err());
    }

    // kagi (T-TERM-INTERACT-001): PtyWrite must be queued for the PTY, not
    // forwarded through the TerminalEvent channel — the channel is only
    // drained on render, which is too slow for a program that queries the
    // terminal and blocks waiting for the answer (root cause of zellij
    // hanging at startup).
    #[test]
    fn test_pty_write_queues_bytes_not_channel() {
        let (tx, rx) = channel();
        let proxy = GpuiEventProxy::new(tx);
        let handle = proxy.pty_responses_handle();

        proxy.send_event(Event::PtyWrite("\x1b[?6c".to_string()));

        // Nothing goes through the TerminalEvent channel for PtyWrite.
        assert!(rx.try_recv().is_err());

        // The bytes land in the queue, verbatim.
        let queued = handle.lock().clone();
        assert_eq!(queued, b"\x1b[?6c");
    }

    #[test]
    fn test_pty_write_queue_accumulates_across_events() {
        let (tx, _rx) = channel();
        let proxy = GpuiEventProxy::new(tx);
        let handle = proxy.pty_responses_handle();

        proxy.send_event(Event::PtyWrite("abc".to_string()));
        proxy.send_event(Event::PtyWrite("def".to_string()));

        assert_eq!(&*handle.lock(), b"abcdef");
    }

    #[test]
    fn test_disconnected_channel() {
        let (tx, rx) = channel();
        let proxy = GpuiEventProxy::new(tx);

        // Drop the receiver to disconnect the channel
        drop(rx);

        // Sending should not panic even though the channel is disconnected
        proxy.send_event(Event::Wakeup);
    }
}
