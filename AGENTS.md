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
   from the UI goes through `kagi::git::Backend` or the free functions in `crates/kagi-git/src/`.
2. **`kagi-domain` stays pure.** No `git2`, no `gpui`, no I/O. Keep
   `crates/kagi-domain/Cargo.toml` dependency-free. All pure-logic unit tests live here.
3. **No destructive commands — ever.** `push --force`, `reset --hard`, and `git clean`
   must not appear anywhere in the codebase. Their absence is the product's reason to exist.
4. **Every write operation follows `plan → confirm → preflight → execute → verify → oplog`.**
   Keep the `plan_X` / `preflight_X` / `execute_X` triple together in the matching
   per-feature module under `crates/kagi-git/src/ops/<feature>.rs`. Never let the UI
   mutate the repo outside this path.

## Layering — where things are allowed to live

| Layer | Location | Rule |
|---|---|---|
| Pure domain | `crates/kagi-domain/` | Models, Graph, Diff, Conflict FSM, Plan types, parsers. No git2/gpui/I/O. |
| Git backend | `crates/kagi-git/` | The **only** place `git2::Repository` is opened. `ops/<feature>.rs` triples, `cli.rs` (fetch/push shell), `backend.rs` (`Backend` facade). |
| UI (View + state) | `src/ui/` | GPUI `Render`, modals, toolbar/sidebar/diff/terminal. No git2. |
| Shell | `src/main.rs`, `src/headless.rs` | Window/menu, bootstrap, test harness. |

Dependency direction: `kagi(bin)` → `ui`(gpui) + `git`(git2) + `kagi-domain`(pure).
`kagi-domain` depends on nothing in this repo.

## File / function size targets

- Aim for ≤ 800 LOC per file. Past that, split on a feature boundary.
- Aim for ≤ 80 LOC per function.
- `src/ui/mod.rs` is a known oversized god-file mid-split; prefer adding new code to a
  focused sibling module over growing it. (`kagi-git`'s ops are already split into
  per-feature `crates/kagi-git/src/ops/<feature>.rs` modules — keep each focused.)

## State-update rules

- Per-tab view data is one **session-owned** read model: `KagiApp::reads`
  (`app::Reads<TabViewState>`), keyed by the `SessionId` of the tab that owns the
  worktree (#482 stage 2 / ADR-0183). There is no `active_view` field and no
  `tab_cache`: switching tabs changes which key is read and copies nothing.
  Adding a field to per-tab data still needs **2 places**: the `TabViewState`
  struct and `build_tab_view`.
  Read it with `self.view().<field>`, update it in place with
  `self.view_mut().<field>` (a status-only change must not rebuild the rows),
  and publish a whole new read with `publish_tab_view` / `accept_tab_view` — a
  background read that is superseded or belongs to a background tab must never
  touch the active tab's panes.
- Modals are a single `active_modal: Option<ActiveModal>` field on `KagiApp`
  (ADR-0093 / ADR-0076; the "one modal at a time" invariant is now structural).
  Adding a modal: add an `ActiveModal` variant in `src/ui/modals.rs`, the
  accessors in `src/ui/operations/modal_state.rs` (`X`/`set_X`/`clear_X`, plus an
  `X_mut` for modals with editable fields), an `open_*`/`cancel_*` method that uses
  `set_X`/`clear_X`, render
  routing, and the entry in `confirm_active_modal`/`cancel_active_modal`. Use the
  accessors — never reach into `active_modal` directly. (`CommitPanel` has its own
  separate `plan_modal`; don't confuse it with the checkout `plan_modal` accessor.)

## Settings (`settings.json`) rules

- Settings live in `crates/kagi-ui-core/src/settings/`, parsed with `serde_json` into
  the typed `Settings` struct (issue #13 P4 / ADR-0091). On disk it stays a **flat
  object of string values** (`"auto_fetch": "true"`, `"ui_zoom": "1000"`) — keep
  writing strings so existing settings files load.
- `settings/store.rs` is the **only** owner of read-modify-write (#491 / ADR-0188):
  the parsed document lives in a process-global store, saves are a same-directory
  temp file + rename, and a `settings.json` that doesn't parse is moved aside to an
  exclusively reserved `settings.json.corrupt[.N]` instead of being overwritten by
  an empty default. Never add a second `fs::write` path to `settings.json`. Only a
  *repeated* write of the same key coalesces (a divider drag); every other write
  lands immediately. Any new process-exit path must call `settings::flush()`.
- `write_setting` round-trips the **whole object**, so unknown keys are preserved — no
  `SETTINGS_KEYS` array to maintain, and adding a key needs no registration.
- Prefer the typed `Settings` accessors (`Settings::load().theme()` / `ui_zoom_permille()`
  / `graph_compact()` / `auto_fetch()`); add a new typed accessor for a new typed read.
  The string `read_setting` / `write_setting` API remains for ad-hoc keys.
- Access only through `settings::` (typed `Settings` or `read_setting`/`write_setting`).
  `theme.rs` is for theme tokens only.

## Error handling

- Git layer returns `Result<T, GitError>` (`crates/kagi-git/src/lib.rs:143`). Avoid `.unwrap()`
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
2. Git operation? Add the `plan_/preflight_/execute_` triple in the matching
   `crates/kagi-git/src/ops/<feature>.rs` module and a matching integration test in `tests/`.
3. UI? Add `open_/confirm_/start_` methods on `KagiApp`; add the modal in
   `src/ui/modals.rs`.
4. i18n: add EN **and** JA strings to the `Msg` enum in `src/ui/i18n.rs`.

## Naming conventions

- Operations: `plan_X` / `preflight_X` / `execute_X` / `verify_X` (keep the triple aligned).
- UI methods: `open_X_modal` / `cancel_X` / `replan_X` / `confirm_X` / `start_X`.
- Domain types are defined in `kagi-domain`; `crates/kagi-git/src/` re-exports them (shim) rather
  than redefining.

## Files whose dependencies you must understand before editing

- `src/ui/mod.rs` (`KagiApp`): 110+ interdependent fields. Grep for callers before changing.
- `crates/kagi-git/src/ops/`: triples share `StateSummary` / `OperationPlan`.
- `docs/rearch/migration/README.md`: confirm which step is done/pending before refactoring.

## Verifying changes

- `cargo build` and `cargo test --workspace` must stay green at every step.
- Native GUI E2E is compile-time opt-in (ADR-0166). Default builds, tests and
  Clippy do not compile the runner or enable `gpui/test-support`. On macOS use:

  ```sh
  KAGI_GUI_E2E=1 \
    cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
  ```

  The feature enables compilation; `KAGI_GUI_E2E=1` permits native execution.
- **Before committing/pushing, run `cargo fmt --all`.** CI's `fmt + clippy` job is
  advisory (non-blocking) but `cargo fmt --check` exits non-zero on any diff, which
  turns the job red. Run `cargo fmt --check` to confirm clean. Also run
  `cargo clippy --workspace` and don't add *new* warnings (pre-existing v0.2.0 debt
  is tolerated; clippy has no `-D warnings`, so warnings alone won't fail CI, but
  keep your own diff clean — annotate justified cases with `#[allow(...)]`).
- The GUI cannot be exercised by subagents — UI-affecting changes need a human (or the
  primary session) to launch the app and eyeball it. Build + tests passing is necessary
  but not sufficient for UI behavior.
- **Repository invariants run through uv, never shell text tools.** The gates live
  in `ci/` as a uv project (`kagi-checks`); run them from the repo root:

  ```
  uv run --project ci check-all              # every gate + the rule selftest
  uv run --project ci check-modal-sections   # one gate
  uv run --project ci check-loc --write-baseline   # accept a ratchet change
  uv run --project ci ruff check ci && uv run --project ci mypy --config-file ci/pyproject.toml
  ```

  Adding an invariant = one `Rule` in `ci/src/kagi_checks/rules.py` (pattern, globs,
  message, plus a `sample` it must flag and a `sample_ok` it must not) and one
  `check-<name>` entry in `ci/pyproject.toml` + the workflow matrix. `check-all
  --selftest` proves every rule still matches its own sample, so a rule that
  silently stops matching fails loudly instead of passing green.
- **Never write a gate as `grep`/`find`/`awk`/`sed -i`, and never invoke `python3`
  directly in a workflow** — `check-shell-hygiene` fails the build if you do. Those
  tools differ between GNU (CI) and BSD (macOS dev machines): BSD grep caps bounded
  repetition at 255, so a valid GNU pattern matches *nothing* and the gate goes green
  having checked zero files. That shipped once (#454) and a human, not CI, caught it.
  Interactive searching is a different matter — use `rg`/`fd` freely there.

## Build hygiene

- Run only one cargo command at a time per tree. Wait for tests to finish
  before starting Clippy, check or another build.
- Every worktree uses Cargo's default, worktree-local `target/` directory. Do not
  set `CARGO_TARGET_DIR` or Cargo's `build.target-dir` to a shared path.
- Do not create or use `.claude/worktrees/.cargo/config.toml` to redirect a
  worktree to `../../target`; its artifacts must remain isolated from the primary
  checkout and other worktrees.
- This isolation is required by the 2026-09-07 slice 1a result: Codex Design and
  PM independently reproduced workspace crate artifacts (`kagi-domain`,
  `kagi-git`, and `kagi`) colliding in a shared target. Cargo then reused a stale
  rlib as Fresh and reported E0432 for a newly added module. Since #520, each
  clean worktree build uses roughly 3–4 GB, so the disk cost is intentional.
- Once a week, when no build is running in that tree, prune artifacts older than
  seven days in every worktree: `cargo sweep --time 7` (requires cargo-sweep).
  Do not routinely clean a worktree target; clean-build benchmarks need an idle
  build window and destroy reusable build artifacts.

## Code Review Rules

### Review language

- Write all user-facing Codex GitHub code-review comments in Japanese.
- Keep code identifiers, commands, file paths, and established technical terms
  in English where that is clearer.
- Write findings, follow-up replies, and approval or no-finding summaries in
  Japanese.
