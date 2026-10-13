//! Resolved platform display preferences; native reads belong to the shell.
use std::sync::atomic::{AtomicU8, Ordering};

static SYSTEM_DISPLAY: AtomicU8 = AtomicU8::new(0);

pub fn system_display() -> (bool, bool) {
    let flags = SYSTEM_DISPLAY.load(Ordering::Relaxed);
    (flags & 1 != 0, flags & 2 != 0)
}

/// Publish a complete system preference snapshot without changing app settings.
pub fn set_system_display(motion: bool, transparency: bool) -> bool {
    let flags = u8::from(motion) | (u8::from(transparency) << 1);
    SYSTEM_DISPLAY.swap(flags, Ordering::Relaxed) != flags
}
