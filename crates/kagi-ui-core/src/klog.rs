//! Headless test-contract logging (`[kagi] …`) — issue #13 Low-1 / ADR-0096.
//!
//! Invariant (ADR-0096): never change the wording or ordering of existing
//! `[kagi]` lines; headless scenarios match stderr exactly as behavior evidence.
//!
//! Every `[kagi] …` line printed to stderr is part of the `KAGI_*` headless
//! test contract: `src/headless.rs` and the integration tests grep stderr for
//! these exact lines, so their format and wording must not change casually
//! (see AGENTS.md "Logging rules"). Routing them all through the [`klog!`] macro
//! makes that contract a single, greppable channel — distinct from ad-hoc
//! human/diagnostic output (plain `eprintln!`/`tracing`), which can evolve
//! freely. This is the seam ADR-0076 / the issue #13 review (P5 / Low-1) call
//! for; output is byte-identical to the previous `eprintln!("[kagi] …")`.
//!
//! Usage: `klog!("refreshed")` or `klog!("plan: {} → {}", from, to)` — the
//! `[kagi] ` prefix is added by the macro; pass only the message.
//!
//! #516: the GUI E2E runner can also keep the last [`TAIL_LINES`] lines in
//! memory ([`keep_tail`]) to write them out as failure evidence. Off by
//! default: then a line costs one atomic load on top of the `eprintln!`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// How many of the most recent lines [`keep_tail`] retains.
pub const TAIL_LINES: usize = 200;

static KEEP_TAIL: AtomicBool = AtomicBool::new(false);
static TAIL: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Start keeping the last [`TAIL_LINES`] lines (the GUI E2E runner, #516).
pub fn keep_tail() {
    KEEP_TAIL.store(true, Ordering::Relaxed);
}

/// Whether lines are being kept; read by [`klog!`].
#[doc(hidden)]
pub fn keeping_tail() -> bool {
    KEEP_TAIL.load(Ordering::Relaxed)
}

/// Keep one printed line, dropping the oldest beyond [`TAIL_LINES`].
#[doc(hidden)]
pub fn keep(line: String) {
    let mut tail = TAIL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if tail.len() == TAIL_LINES {
        tail.pop_front();
    }
    tail.push_back(line);
}

/// The kept lines, oldest first. Empty unless [`keep_tail`] was called.
pub fn tail() -> Vec<String> {
    let tail = TAIL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    tail.iter().cloned().collect()
}

/// Emit one headless test-contract log line (`[kagi] <message>`) to stderr.
#[macro_export]
macro_rules! klog {
    ($($arg:tt)*) => {
        if $crate::klog::keeping_tail() {
            let line = format!("[kagi] {}", format_args!($($arg)*));
            eprintln!("{line}");
            $crate::klog::keep(line);
        } else {
            eprintln!("[kagi] {}", format_args!($($arg)*))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test owns the process-global tail: enabling it is one-way.
    #[test]
    fn the_tail_keeps_the_last_lines_in_order() {
        assert!(tail().is_empty(), "nothing kept before it is enabled");
        klog!("dropped {}", 0);
        assert!(tail().is_empty(), "a line before enabling is not kept");

        keep_tail();
        for n in 0..TAIL_LINES + 5 {
            klog!("line {}", n);
        }
        let kept = tail();
        assert_eq!(kept.len(), TAIL_LINES, "bounded");
        let newest = format!("[kagi] line {}", TAIL_LINES + 4);
        assert!(kept.contains(&newest), "the newest line is kept");
        // Line 4 was followed by TAIL_LINES newer lines of ours, so it is out
        // however other tests' lines interleave.
        assert!(!kept
            .iter()
            .any(|l| l == "[kagi] line 4" || l == "[kagi] dropped 0"));
    }
}
