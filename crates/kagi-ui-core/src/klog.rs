//! Headless test-contract logging (`[kagi] …`) — issue #13 Low-1 / ADR-0096.
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

/// Emit one headless test-contract log line (`[kagi] <message>`) to stderr.
#[macro_export]
macro_rules! klog {
    ($($arg:tt)*) => {
        eprintln!("[kagi] {}", format_args!($($arg)*))
    };
}
