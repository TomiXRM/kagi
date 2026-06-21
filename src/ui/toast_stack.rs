//! Toast notification stack — self-contained logic (ADR-0110 / Phase 5 start).
//!
//! Extracted from `KagiApp` so the toast add/remove/expire/ticker logic lives
//! in one testable struct. Previously this was ~80 lines of inline methods on
//! the 104-field `KagiApp` god-struct, interleaved with unrelated state.
//!
//! Today the stack is held as `Rc<RefCell<ToastStack>>` on `KagiApp` (not an
//! `Entity<T>`) because `push_toast` is called from ~38 sites that don't all
//! have `cx`. When those callers are refactored to thread `cx`, this becomes
//! `Entity<ToastStack>` and each push/expire only re-renders the overlay, not
//! the whole app — the full Phase 5 win. Even as `Rc<RefCell>`, the separation
//! pays off: one struct, one responsibility, unit-testable in isolation.

use std::time::Instant;

use gpui::SharedString;

use crate::ui::types::{Toast, ToastKind};

/// Maximum simultaneous toasts; oldest is dropped beyond this.
pub const TOASTS_MAX: usize = 4;
/// Slide-out animation duration before final removal (ms).
pub const TOAST_REMOVE_MS: u64 = 300;

/// Self-contained toast stack. All toast lifecycle logic lives here.
pub struct ToastStack {
    toasts: Vec<Toast>,
    next_id: u64,
}

impl ToastStack {
    pub fn new() -> Self {
        Self {
            toasts: Vec::new(),
            next_id: 1,
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
