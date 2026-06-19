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

## State-update rules

- Per-tab view data is a single `active_view: TabViewState` field on `KagiApp`
  (ADR-0075 P2 / ADR-0095). Inactive tabs live in `tab_cache`. Adding a field to
  per-tab data needs **2 places**: the `TabViewState` struct and `build_tab_view`
  (builds it from a snapshot). `apply_tab_view` is a whole-struct move, so it no
  longer has to be updated — and the field can't silently vanish on tab switch.
  Read active per-tab data via `self.active_view.<field>`.
- Modals are a single `active_modal: Option<ActiveModal>` field on `KagiApp`
  (ADR-0093 / ADR-0076; the "one modal at a time" invariant is now structural).
  Adding a modal: add an `ActiveModal` variant in `src/ui/modals.rs`, the five
  accessors in `src/ui/operations/modal_state.rs` (`X`/`X_mut`/`set_X`/`clear_X`/
  `take_X`), an `open_*`/`cancel_*` method that uses `set_X`/`clear_X`, render
  routing, and the entry in `confirm_active_modal`/`cancel_active_modal`. Use the
  accessors — never reach into `active_modal` directly. (`CommitPanel` has its own
  separate `plan_modal`; don't confuse it with the checkout `plan_modal` accessor.)

## Settings (`settings.json`) rules

- Settings live in `src/ui/settings.rs`, parsed with `serde_json` into the typed
  `Settings` struct (issue #13 P4 / ADR-0091). On disk it stays a **flat object of
  string values** (`"auto_fetch": "true"`, `"ui_zoom": "1000"`) — keep writing strings
  so existing settings files load.
- `write_setting` round-trips the **whole object**, so unknown keys are preserved — no
  `SETTINGS_KEYS` array to maintain, and adding a key needs no registration.
- Prefer the typed `Settings` accessors (`Settings::load().theme()` / `ui_zoom_permille()`
  / `graph_compact()` / `auto_fetch()`); add a new typed accessor for a new typed read.
  The string `read_setting` / `write_setting` API remains for ad-hoc keys.
- Access only through `settings::` (typed `Settings` or `read_setting`/`write_setting`).
  `theme.rs` is for theme tokens only.

## Error handling

- Git layer returns `Result<T, GitError>` (`src/git/mod.rs:137`). Avoid `.unwrap()`
  outside tests.
- User-facing errors must surface via the oplog **and** a modal — never swallowed.

## Logging rules (important — read before touching any log line)

- The `[kagi] …` lines are a **test contract**. The `KAGI_*` headless harness
  (`src/headless.rs`) greps stderr to verify behavior. Do not change the format,
  wording, or ordering of existing `[kagi]` lines.
- Emit every contract line through the **`klog!`** macro (`src/klog.rs`, ADR-0096):
  `klog!("refreshed")`, `klog!("plan: {} → {}", a, b)` — the `[kagi] ` prefix is
  added by the macro. This is the single, greppable contract channel.
- Use plain `eprintln!`/`tracing` only for ad-hoc human/diagnostic output — never the
  `[kagi]` prefix by hand, and never route a `klog!` contract line through `eprintln!`.
- New features that need headless coverage add `klog!` lines in the established format
  (see `docs/tickets/` T-* specs). Do not "clean up" or reword existing contract lines.

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
- **Before committing/pushing, run `cargo fmt --all`.** CI's `fmt + clippy` job is
  advisory (non-blocking) but `cargo fmt --check` exits non-zero on any diff, which
  turns the job red. Run `cargo fmt --check` to confirm clean. Also run
  `cargo clippy --workspace` and don't add *new* warnings (pre-existing v0.2.0 debt
  is tolerated; clippy has no `-D warnings`, so warnings alone won't fail CI, but
  keep your own diff clean — annotate justified cases with `#[allow(...)]`).
- The GUI cannot be exercised by subagents — UI-affecting changes need a human (or the
  primary session) to launch the app and eyeball it. Build + tests passing is necessary
  but not sufficient for UI behavior.
