# ADR-0096: `klog!` — a single channel for the `[kagi]` test contract (issue #13 Low-1)

**Status:** Accepted — 2026-06-17
**Refs:** ADR-0076 / ADR-0094 (VM layer), issue #13 (P5 / Low-1)

## Context

Every `[kagi] …` line on stderr is part of the `KAGI_*` headless test contract:
`src/headless.rs` and the integration tests grep stderr for these exact lines.
They were emitted by ~347 hand-written `eprintln!("[kagi] …")` calls scattered
across the UI. That made the contract implicit and indistinguishable from ad-hoc
human/diagnostic `eprintln!`s — the review (P5 / Low-1) flagged the double duty
as a hazard: you cannot tell a contract line from a stray debug print, and you
cannot evolve human logs without risking the tests.

ADR-0076 / the review sequence the *full* log/VM split for later; this ADR takes
the safe first step — establishing the **seam**.

## Decision

Add a `klog!` macro (`src/klog.rs`, `#[macro_use]` in `main.rs`) that emits one
`[kagi] <message>` line to stderr, and route **all** contract lines through it:

```rust
klog!("refreshed");
klog!("plan: {} → {}", from, to);
```

The `[kagi] ` prefix is owned by the macro (the single emission point). All ~347
call sites were converted mechanically (`eprintln!("[kagi] …")` → `klog!("…")`);
output is **byte-identical**, so the headless contract is unchanged.

## Consequences

- The contract is now one greppable channel (`klog!(`), distinct from ad-hoc
  `eprintln!`/`tracing` human output — which can evolve without touching tests.
- A future change to *how* contract lines are emitted (structured output, a test
  sink, routing onto VM unit tests per ADR-0094) has exactly one place to change.
- Behaviour preserved: `cargo test --workspace` = 743 passed / 0 failed (the
  headless tests assert the exact stderr lines); git2 gate, fmt, clippy clean.

## Not done

The deeper half of P5 — moving headless *assertions* off stderr-grep and onto VM
unit tests so the `KAGI_*` harness can shrink — continues from the `view_models`
foundation (ADR-0094). `klog!` is the emission seam that makes that migration
incremental.
