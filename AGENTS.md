# Kagi — Agent Development Rules

Kagi is a safety-first, commit-graph Git GUI built with GPUI (Rust). This file is
the single entry point for AI agents (and humans) working in this repo. Read it
before editing. It encodes invariants that are otherwise scattered across 90+ ADRs.

## Required reading (before any work)

- `docs/rearch/architecture.md` — the v1.0 target architecture (crate split).
- `docs/rearch/migration/README.md` — current position in the strangler plan (S1–S7).
- `docs/adr/` — read the ADR for the feature you touch; write one for new decisions.

## Invariants (violating any of these = the change is wrong)

1. **No `git2::` in `src/ui/`.** `Repository::open` is also forbidden there. This is
   enforced by a CI grep gate (`.github/workflows/ci.yml`, ADR-0078). All Git access
   from the UI goes through `kagi::git::Backend` or the free functions in `src/git/`.
2. **`kagi-domain` stays pure.** No `git2`, no `gpui`, no I/O. Keep
   `crates/kagi-domain/Cargo.toml` dependency-free. All pure-logic unit tests live here.
3. **No destructive commands — ever.** `push --force`, `reset --hard`, and `git clean`
   must not appear anywhere in the codebase. Their absence is the product's reason to exist.
4. **Every write operation follows `plan → confirm → preflight → execute → verify → oplog`.**
   Keep the `plan_X` / `preflight_X` / `execute_X` triple together in `src/git/ops.rs`
   (future: `src/git/ops/`). Never let the UI mutate the repo outside this path.

## Layering — where things are allowed to live

| Layer | Location | Rule |
|---|---|---|
| Pure domain | `crates/kagi-domain/` | Models, Graph, Diff, Conflict FSM, Plan types, parsers. No git2/gpui/I/O. |
| Git backend | `src/git/` | The **only** place `git2::Repository` is opened. `ops.rs` triples, `cli.rs` (fetch/push shell), `backend.rs` (`Backend` facade). |
| UI (View + state) | `src/ui/` | GPUI `Render`, modals, toolbar/sidebar/diff/terminal. No git2. |
| Shell | `src/main.rs`, `src/headless.rs` | Window/menu, bootstrap, test harness. |

Dependency direction: `kagi(bin)` → `ui`(gpui) + `git`(git2) + `kagi-domain`(pure).
`kagi-domain` depends on nothing in this repo.

## File / function size targets

- Aim for ≤ 800 LOC per file. Past that, split on a feature boundary.
- Aim for ≤ 80 LOC per function.
- `src/ui/mod.rs` and `src/git/ops.rs` are known oversized god-files mid-split;
  prefer adding new code to a focused sibling module over growing them.

## State-update rules (current reality — see ADR-0075 for the planned fix)

- Adding a field to per-tab view data requires updating **3 places**: the
  `TabViewState` struct, `build_tab_view` (builds it from a snapshot), and
  `apply_tab_view` (copies it into the active `KagiApp` fields). Miss one and the
  field silently vanishes on tab switch.
- Adding a modal currently requires 4 touch points: a `KagiApp` field, an `open_*`
  method, a `cancel_*` method, and render routing. (ADR-0076 plans an `ActiveModal`
  enum to collapse these — until then, follow the existing pattern exactly.)

## Settings (`settings.json`) rules

- Settings live in `src/ui/settings.rs` — `read_setting` / `write_setting` /
  `settings_path` / `parse_string_value`, and the `SETTINGS_KEYS` array which is the
  source of truth for which keys persist.
- **Adding a key requires adding it to `SETTINGS_KEYS`** — a key not in the array is
  dropped on the next save of any other key.
- The parser is hand-written flat string-KV (not serde): bool/number values are stored
  as strings (`"1"`/`"0"`). Don't assume typed values.
- Access only through `read_setting`/`write_setting`. `theme.rs` is for theme tokens only.

## Error handling

- Git layer returns `Result<T, GitError>` (`src/git/mod.rs:137`). Avoid `.unwrap()`
  outside tests.
- User-facing errors must surface via the oplog **and** a modal — never swallowed.

## Logging rules (important — read before touching any `eprintln!`)

- `[kagi]` prefixed `eprintln!` lines are a **test contract**. The `KAGI_*` headless
  harness (`src/headless.rs`) greps stderr to verify behavior. Do not change the
  format, wording, or ordering of existing `[kagi]` lines.
- New features that need headless coverage must add `[kagi]` log lines in the same
  established format (see `docs/tickets/` T-* specs).
- Do not "clean up" or normalize these logs. (A proper VM/logging split is deferred —
  see issue #13 / ADR roadmap.)

## Adding a new feature

1. Read or write the relevant ADR in `docs/adr/`.
2. Git operation? Add the `plan_/preflight_/execute_` triple in `src/git/ops.rs` and a
   matching integration test in `tests/`.
3. UI? Add `open_/confirm_/start_` methods on `KagiApp`; add the modal in
   `src/ui/modals.rs`.
4. i18n: add EN **and** JA strings to the `Msg` enum in `src/ui/i18n.rs`.

## Naming conventions

- Operations: `plan_X` / `preflight_X` / `execute_X` / `verify_X` (keep the triple aligned).
- UI methods: `open_X_modal` / `cancel_X` / `replan_X` / `confirm_X` / `start_X`.
- Domain types are defined in `kagi-domain`; `src/git/` re-exports them (shim) rather
  than redefining.

## Files whose dependencies you must understand before editing

- `src/ui/mod.rs` (`KagiApp`): 110+ interdependent fields. Grep for callers before changing.
- `src/git/ops.rs`: triples share `StateSummary` / `OperationPlan`.
- `docs/rearch/migration/README.md`: confirm which step is done/pending before refactoring.

## Verifying changes

- `cargo build` and `cargo test --workspace` must stay green at every step.
- The GUI cannot be exercised by subagents — UI-affecting changes need a human (or the
  primary session) to launch the app and eyeball it. Build + tests passing is necessary
  but not sufficient for UI behavior.
