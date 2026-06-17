# ADR-0094: View-model layer foundation — `StatusBarVM` (issue #13 P5)

**Status:** Accepted — 2026-06-17
**Refs:** ADR-0076 (design), ADR-0077 (test pyramid), issue #13 (P5 / Medium-3)

## Context

ADR-0076 calls for a view-model (VM) layer: plain-data projections of repo
state that views render and that can be **unit-tested without a window**, so the
`KAGI_*` headless harness (issue #13 P5) can be retired over time. The pieces
were already half-present — `StatusBarSummary`/`ToolbarState` are pure snapshots
with `from_snapshot`, and `build_tab_view` is a pure function — but presentation
*decisions* still lived inline in the 6k-LOC `render.rs` (e.g. which status-bar
chips to show and their exact labels), untestable without rendering.

## Decision

Introduce `src/ui/view_models/` and land the first VM, `StatusBarVM`:

- `StatusBarVM::from_summary(&StatusBarSummary)` produces the ordered list of
  status-bar chips (`StatusChip { role, text }`) with the exact historical
  visibility rules and labels (`●`, `+N`, `~N`, `!N`, `⚑N`, `↑A ↓B`,
  `no upstream`, `→ origin/main`).
- `render_status_bar` now builds the VM and maps each `StatusChipRole` to a
  theme colour + margin — the view keeps only the `gpui` assembly; the *decision*
  logic moved into the pure, tested VM.
- The VM is plain data (no `gpui`, no `git2`) and is unit-tested in-module
  without a window (4 tests covering clean/dirty/no-upstream/detached).

This is a behaviour-preserving extraction and the seed of the layer; further VMs
(`CommitGraphVM`, `InspectorVM`, `DiffVM`, `SidebarVM`, …) follow the same shape
incrementally, as ADR-0076 envisions ("段階導入").

## Consequences

- Status-bar presentation is now verifiable with fast, window-free unit tests —
  a concrete step toward reducing `KAGI_*` headless coverage (P5 goal).
- The pattern (pure VM + thin view + in-module tests) is established for the rest
  of `render.rs` to follow.
- Behaviour preserved: `cargo test --workspace` = 743 passed / 0 failed (739 +
  4 new VM tests); git2 gate, fmt, clippy clean.

## Not done (continues under P5)

- Remaining VMs for the graph, inspector, diff, sidebar, commit panel, conflict
  views.
- The log-protocol split (issue #13 Low-1): separating the `[kagi]` test-contract
  lines from human logs. ADR-0076/the review sequence this *after* the VM layer
  is broad enough that headless assertions can move onto VM unit tests; until
  then the `[kagi]` lines remain the contract and must not be reformatted.
