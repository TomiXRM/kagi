# ADR-0093: `ActiveModal` enum — implementing ADR-0076 (issue #13 P7)

**Status:** Accepted — 2026-06-17
**Refs:** ADR-0076 (design), issue #13 (P7, "Medium-2")

## Context

`KagiApp` held ~22 mutually-exclusive `Option<XModal>` fields
(`plan_modal`, `pull_modal`, `merge_modal`, …). The "at most one modal is open
at a time" invariant was enforced only by convention and by long `if … is_some()
else if …` chains in `confirm_active_modal` / `cancel_active_modal`. The issue
#13 review (P7) and ADR-0076 proposed collapsing them into a single enum so the
invariant is structural.

## Decision

Replace the 22 fields with one `active_modal: Option<ActiveModal>`, where
`ActiveModal` (defined in `src/ui/modals.rs`) has one variant per modal wrapping
its existing payload struct:

```rust
pub enum ActiveModal {
    Checkout(CheckoutPlanModal),
    Pull(PullPlanModal),
    // … 22 variants total …
}
```

Ergonomic per-modal accessors live in `src/ui/operations/modal_state.rs`
(`impl KagiApp`): for each modal `X` of type `T`/variant `V` —
`X(&self) -> Option<&T>`, `X_mut`, `set_X(T)`, `clear_X()` (clears only if `X` is
active, matching the old `self.X = None`), and `take_X()`. Call sites use these
instead of the old fields, so the change is a representation swap, not a
behaviour change. `confirm_active_modal` / `cancel_active_modal` keep their exact
priority order.

Out of scope / unchanged: `CommitPanel` has its **own** `plan_modal` field (a
different type) — left untouched; and the non-modal overlays
`smart_commit.modal`, `update_modal_open`, `menu_overlay` stay as-is.

## Consequences

- The one-modal-at-a-time invariant is now expressed in the type system; opening
  a modal (`set_X`) implicitly supersedes any other, and there is a single place
  ("`active_modal`") to inspect modal state.
- Adding a modal = one enum variant + five generated accessors, instead of a new
  field plus remembering to clear it everywhere.
- Behaviour preserved: `cargo test --workspace` = 739 passed / 0 failed
  (including the `KAGI_*` headless modal tests); git2 gate, fmt, clippy clean.

This implements the ActiveModal half of ADR-0076; the View-Model-layer half of
ADR-0076 remains tracked separately (issue #13 P5).
