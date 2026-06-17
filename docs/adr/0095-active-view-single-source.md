# ADR-0095: Collapse per-tab view dual-state into `active_view` (issue #13 P2)

**Status:** Accepted — 2026-06-17
**Refs:** ADR-0075 (RepoSession design), issue #13 (P2)

## Context

`KagiApp` held the active tab's snapshot-derived display data as ~16 top-level
fields (`rows`, `details`, `branches`, `status_summary`, `toolbar_state`, …) —
the *same* fields that also make up `TabViewState`, which is what `tab_cache`
stores for inactive tabs. `apply_tab_view` bridged the two with a hand-written
field-by-field copy. The issue #13 review (P2) flagged this as the worst
state-duplication: adding a field to `TabViewState` silently required editing
`apply_tab_view` too, or the field would vanish on tab switch (it would be in the
cache but never copied into the active fields).

## Decision

Replace the ~16 duplicated top-level fields with a single
`active_view: TabViewState` on `KagiApp`. Consequences:

- `apply_tab_view` becomes one move: `self.active_view = view;`. There is no
  per-field copy to forget.
- The per-tab field set is defined in exactly one place (`TabViewState`); adding
  a field touches `TabViewState` + `build_tab_view` only.
- Call sites read active data via `self.active_view.<field>` (mechanical change,
  ~125 sites; `StatusBarSummary`'s own `is_dirty` field was left untouched).
- `tab_cache: HashMap<PathBuf, TabViewState>` is unchanged; inactive tabs still
  cache their `TabViewState`.

This is the behaviour-preserving slice of ADR-0075 that removes the P2
foot-gun. The full ADR-0075 model (a `Vec<RepoSession>` + active index with a
zero-copy `active = idx` swap, plus `OperationController`) remains the larger
v1.0 target; `active_view` is the intermediate step that already eliminates the
dual-definition and the `apply_tab_view` sync hazard.

## Consequences

- Adding per-tab view data can no longer desync on tab switch.
- One obvious place (`active_view`) holds the active tab's derived data.
- Behaviour preserved: `cargo test --workspace` = 743 passed / 0 failed; git2
  gate, fmt, clippy clean.

## Not done

The full `RepoSession`/`OperationController` model (ADR-0075) — `active_view`
still holds a *copy* of the active tab's view rather than indexing into a single
per-session store; collapsing that copy into a pure `active = idx` swap is the
remaining v1.0 work.
