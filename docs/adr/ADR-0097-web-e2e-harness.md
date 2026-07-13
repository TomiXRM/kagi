# ADR-0097: Browser (gpui_web + Playwright) UI e2e harness

## Status

Accepted (2026-07-14)

## Context

The zed-main gpui migration (PR #125) surfaced a layout bug class that unit
tests and the `[kagi]` headless harness cannot see: frame-level layout
oscillation while a pane is resized (wrapped-text measure lag). Verifying it
required a human dragging a divider. Upstream now ships `gpui_web`, and
longbridge's story-web proves the WASM path works in browsers — where
Playwright can drive resizes and take screenshots headlessly.

## Decision

- New workspace crate **`crates/kagi-web`** (cdylib): a *catalog* of kagi UI
  stories rendered with `gpui_platform::single_threaded_web()`. One story per
  known layout-regression class; the first replicates the inspector
  header/message layout from the resize-jitter fix.
- **git2 never targets WASM**, so the catalog only renders UI driven by
  `kagi-domain` data (pure). The Backend stays out by design — this is a UI
  rendering harness, not an app port. Pure presentation helpers move to
  `kagi-domain` when the catalog needs them (first: `message::reflow_message`).
- **`scripts/build-web.sh`** builds the WASM bundle (nightly toolchain —
  `wasm_thread` needs `#![feature]` — plus `wasm-bindgen-cli`) into
  `crates/kagi-web/dist/`.
- **`e2e/`** holds the Playwright suite. Headless Chromium gets WebGPU via
  SwiftShader flags, so the suite is machine-independent and CI-able. Specs:
  boot-without-errors, and a resize sweep asserting layout settles within one
  frame per step.

## Consequences

- UI layout regressions in the browser rendering path are caught by CI-able
  automation instead of manual drag testing.
- Caveat: platform-specific behavior (macOS text system measure cache, the
  original jitter's trigger) does not necessarily reproduce under gpui_web —
  the harness complements, not replaces, native eyeballing.
- The catalog grows one story per fixed layout bug (same policy as `klog!`
  contract lines for behavior).
