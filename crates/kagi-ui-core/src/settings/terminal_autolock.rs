//! Runtime flag for the terminal auto-lock opt-in (#772 / ADR-0206). Same
//! shape as `theme::auto_fetch` / `reduce_motion`, kept under `settings` per
//! the "theme.rs is for theme tokens" rule.

use super::{write_setting, Settings};
use std::sync::atomic::{AtomicBool, Ordering};

/// #772 / ADR-0206: terminal auto-lock opt-in. Default **off**; read when a
/// terminal starts or its shell exits.
static TERMINAL_AUTO_LOCK: AtomicBool = AtomicBool::new(false);

#[inline]
pub fn terminal_auto_lock() -> bool {
    TERMINAL_AUTO_LOCK.load(Ordering::Relaxed)
}

/// Set + persist the terminal auto-lock flag (`terminal_auto_lock`).
pub fn set_terminal_auto_lock(on: bool) {
    TERMINAL_AUTO_LOCK.store(on, Ordering::Relaxed);
    write_setting(
        "terminal_auto_lock",
        Some(if on { "true" } else { "false" }),
    );
}

/// Initialise the terminal auto-lock flag at startup from `settings.json`.
pub fn init_terminal_auto_lock() {
    TERMINAL_AUTO_LOCK.store(Settings::load().terminal_auto_lock(), Ordering::Relaxed);
    klog!("terminal_auto_lock: {}", terminal_auto_lock());
}
