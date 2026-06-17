# ADR-0092: serde-backed `settings.json` (issue #13 P4, second half)

**Status:** Accepted — 2026-06-17
**Refs:** issue #13 (P4), ADR-0091 (deferred "serde `Settings`")

## Context

`settings.json` was read and written by a hand-rolled scanner in
`src/ui/settings.rs` (`parse_string_value`) plus a `SETTINGS_KEYS` array. The
GLM5.2 Max review (issue #13, P4) flagged two foot-guns:

1. **Key loss.** `write_setting` re-read only the keys listed in `SETTINGS_KEYS`,
   so any key not on that list (an older/newer build's key, a hand-added one) was
   silently dropped whenever a sibling key was saved.
2. **Stringly-typed, fragile parsing.** Substring scanning with no real JSON
   parser; bool/number values stored as strings with ad-hoc coercion repeated at
   each call site.

ADR-0091 deferred the fix as "serde `Settings`".

## Decision

Parse and serialize `settings.json` with `serde_json` into a typed `Settings`
struct (`#[serde(flatten)]` over a `serde_json::Map`), and expose typed
accessors (`theme()`, `ui_zoom_permille()`, `graph_compact()`, `auto_fetch()`)
that apply the historical coercions in one place. `theme.rs`'s startup reads now
go through them.

- **On-disk format is unchanged**: still a flat object whose values are JSON
  strings (`"auto_fetch": "true"`, `"ui_zoom": "1000"`). Existing user settings
  files keep loading; no migration needed.
- `write_setting` now round-trips the **entire** object → unknown keys are
  preserved. `SETTINGS_KEYS` and `parse_string_value` are removed (no longer
  load-bearing).
- The string `read_setting` / `write_setting` API is retained (now thin wrappers
  over `Settings`) so existing callers are untouched.
- `serde` + `serde_json` added as direct deps of the `kagi` crate (already in the
  tree via gpui). `kagi-domain` stays dependency-free (invariant intact).

## Consequences

- Adding a setting no longer requires touching a key registry, and a future
  build's keys won't be clobbered by an older build.
- Reads are typed and unit-testable (`Settings` tests cover coercions and
  unknown-key preservation).
- Cost: one new direct dependency in `src/ui` (not the domain layer); a `Settings`
  load reads the whole file (it is tiny, and was already fully re-read per write).
- Behaviour preserved: `cargo test --workspace` = 739 passed / 0 failed; git2 gate
  clean; fmt/clippy clean.

## Not done

Full typed schema (every key as a native-typed field with `bool`/`f32` on disk)
is intentionally out of scope — it would change the on-disk representation and
break backward compatibility for marginal benefit.
