//! Toast notification stack — self-contained logic (ADR-0110 / Phase 5 start).
//!
//! Extracted from `KagiApp` so the toast add/remove/expire/ticker logic lives
//! in one testable struct. Previously this was ~80 lines of inline methods on
//! the 104-field `KagiApp` god-struct, interleaved with unrelated state.
//!
//! Held as an `Entity<ToastStack>` on `KagiApp` (ADR-0110 Phase 5): the cards
//! render via `impl Render for ToastStack` (in `render.rs`) and every
//! push/expire re-renders only this entity's subtree, not the whole app. The
//! auto-dismiss ticker and slide-out timers live here too (`ensure_ticker` /
//! `begin_exit`) so the lifecycle is self-contained. The pure data methods
//! (`push` / `start_exit` / `remove`) stay `cx`-free for unit tests.

use std::time::{Duration, Instant};

use gpui::{Context, SharedString};

use crate::ui::types::{Toast, ToastKind};

/// Maximum simultaneous toasts; oldest is dropped beyond this.
pub const TOASTS_MAX: usize = 4;
/// Slide-out animation duration before final removal (ms).
pub const TOAST_REMOVE_MS: u64 = 300;

/// Self-contained toast stack. All toast lifecycle logic lives here.
pub struct ToastStack {
    toasts: Vec<Toast>,
    next_id: u64,
    /// True while the auto-dismiss ticker task is running (so we never spawn a
    /// second one). Reset when the stack drains.
    ticker_alive: bool,
}

impl ToastStack {
    pub fn new() -> Self {
        Self {
            toasts: Vec::new(),
            next_id: 1,
            ticker_alive: false,
        }
    }

    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// Push a new toast. If over `TOASTS_MAX`, the oldest is dropped.
    pub fn push(&mut self, kind: ToastKind, message: impl Into<SharedString>) {
        let id = self.next_id;
        self.next_id += 1;
        self.toasts.push(Toast {
            id,
            kind,
            message: message.into(),
            born: Instant::now(),
            dismissing: None,
        });
        if self.toasts.len() > TOASTS_MAX {
            self.toasts.remove(0);
        }
    }

    /// Begin sliding a toast out (× button or auto-expiry). The caller schedules
    /// the actual removal after `TOAST_REMOVE_MS`. No-op if gone or already leaving.
    pub fn start_exit(&mut self, id: u64) {
        let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if toast.dismissing.is_some() {
            return;
        }
        toast.dismissing = Some(Instant::now());
    }

    /// Remove a toast by id (after the exit animation has played).
    pub fn remove(&mut self, id: u64) {
        self.toasts.retain(|t| t.id != id);
    }

    /// Return ids of toasts that have hit their lifetime and should start exiting.
    pub fn expiring_ids(&self) -> Vec<u64> {
        self.toasts
            .iter()
            .filter(|t| t.should_start_exit())
            .map(|t| t.id)
            .collect()
    }

    /// True if at least one toast is still counting down (not yet sliding out).
    /// The ticker uses this to decide when to stop.
    pub fn has_pending(&self) -> bool {
        self.toasts.iter().any(|t| t.dismissing.is_none())
    }

    // ── Entity orchestration (cx-driven) ─────────────────────────────────────
    // These wrap the pure data methods with `cx.notify()` + timer tasks so the
    // overlay re-renders in isolation (ADR-0110 Phase 5).

    /// Push a toast, (re)start the auto-dismiss ticker, and re-render this
    /// entity's subtree only.
    pub fn push_notify(
        &mut self,
        kind: ToastKind,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.push(kind, message);
        self.ensure_ticker(cx);
        cx.notify();
    }

    /// Begin sliding a toast out (× button or auto-expiry), then remove it once
    /// the exit animation has played. Re-renders only this entity.
    pub fn begin_exit(&mut self, id: u64, cx: &mut Context<Self>) {
        self.start_exit(id);
        cx.notify();
        cx.spawn(async move |this, acx| {
            acx.background_executor()
                .timer(Duration::from_millis(TOAST_REMOVE_MS))
                .await;
            let _ = this.update(acx, |stack, cx| {
                stack.remove(id);
                cx.notify();
            });
        })
        .detach();
    }

    /// Spawn the 150ms auto-dismiss ticker if toasts are pending and it is not
    /// already running. The task exits as soon as the stack drains.
    fn ensure_ticker(&mut self, cx: &mut Context<Self>) {
        if self.is_empty() || !self.has_pending() || self.ticker_alive {
            return;
        }
        self.ticker_alive = true;
        cx.spawn(async move |this, acx| loop {
            acx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let finished = this.update(acx, |stack, cx| {
                for id in stack.expiring_ids() {
                    stack.begin_exit(id, cx);
                }
                if !stack.has_pending() {
                    stack.ticker_alive = false;
                    true
                } else {
                    false
                }
            });
            match finished {
                Ok(true) | Err(_) => break,
                Ok(false) => {}
            }
        })
        .detach();
    }
}

impl Default for ToastStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_cap() {
        let mut stack = ToastStack::new();
        for i in 0..(TOASTS_MAX + 2) {
            stack.push(ToastKind::Info, format!("msg {i}"));
        }
        assert_eq!(stack.toasts().len(), TOASTS_MAX);
        // Oldest should have been dropped.
        assert!(!stack.toasts().iter().any(|t| t.message == "msg 0"));
    }

    #[test]
    fn start_exit_is_idempotent() {
        let mut stack = ToastStack::new();
        stack.push(ToastKind::Success, "ok");
        let id = stack.toasts()[0].id;
        stack.start_exit(id);
        assert!(stack.toasts()[0].dismissing.is_some());
        // Second call is a no-op.
        let first = stack.toasts()[0].dismissing;
        stack.start_exit(id);
        assert_eq!(stack.toasts()[0].dismissing, first);
    }

    #[test]
    fn remove_by_id() {
        let mut stack = ToastStack::new();
        stack.push(ToastKind::Error, "err");
        let id = stack.toasts()[0].id;
        assert_eq!(stack.toasts().len(), 1);
        stack.remove(id);
        assert!(stack.is_empty());
    }
}
