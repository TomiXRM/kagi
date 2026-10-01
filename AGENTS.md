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
   enforced by the `ui-git2` rule of `uv run --project ci check-all` (ADR-0078). All
   Git access from the UI goes through `kagi_git::Backend` or the free functions in
   `crates/kagi-git/src/` (reach the crate as `kagi_git::`, not `kagi::git::`).
2. **`kagi-domain` stays pure.** No `git2`, no `gpui`, no I/O. Keep
   `crates/kagi-domain/Cargo.toml` dependency-free. All pure-logic unit tests live here.
3. **No destructive commands — ever.** `push --force`, `reset --hard`, and `git clean`
   must not appear anywhere in the codebase. Their absence is the product's reason to exist.
4. **Every write operation follows `plan → confirm → preflight → execute → verify → oplog`.**
   A *write* is anything that changes observable repository state: refs, objects, the
   index's **staged content** (which paths are staged, at which blob OID and mode),
   the working tree, or config. Refreshing the `stat` cache of existing index
   entries is **not** a write (ADR-0193), but the exemption holds only where both
   are true: a test proves every entry's `(path, OID, mode)` survives the refresh
   unchanged, and the repair is opt-in on the UI refresh path. `working_tree_status`
   stays a pure read — it is reached from 100+ `plan_*`/`preflight_*`/snapshot call
   sites, `plan_create_branch` among them, which run before the user confirms.
   Writing **unreferenced objects** at plan time (`git replay` in print mode,
   `git history --dry-run`, ADR-0211 §4.1) is likewise not a write: no ref, index
   or worktree observes them and `gc` reclaims them if the user cancels.
   Keep the `plan_X` / `preflight_X` / `execute_X` triple together in the matching
   per-feature module under `crates/kagi-git/src/ops/<feature>.rs`. Never let the UI
   mutate the repo outside this path.

## Layering — where things are allowed to live

| Layer | Location | Rule |
|---|---|---|
| Pure domain | `crates/kagi-domain/` | Models, Graph, Diff, Conflict FSM, Plan types, parsers. No git2/gpui/I/O. |
| Git backend | `crates/kagi-git/` | The **only** place `git2::Repository` is opened. `ops/<feature>.rs` triples, `cli.rs` (fetch/push shell), `backend.rs` (`Backend` facade), `oplog/`. |
| Application layer | `src/app/` | Operation lifecycle, sessions, reads, reconcile (#484). No UI and no direct I/O — the `app-layering` gate checks it. |
| UI core | `crates/kagi-ui-core/` | Settings store, i18n (`Msg`), `klog!`, theme tokens, shared UI types. |
| UI (View + state) | `src/ui/`, `crates/kagi-ui-*` | GPUI `Render`, modals, toolbar/sidebar/diff/terminal. No git2. |
| MCP | `crates/kagi-mcp/` | MCP server. No gpui (`mcp-gpui` gate). |
| Shell | `src/main.rs`, `src/headless.rs` | Window/menu, bootstrap, test harness. |

Dependency direction: `kagi(bin)` → `ui`(gpui) + `git`(git2) + `kagi-domain`(pure).
`kagi-domain` depends on nothing in this repo.

## File / function size targets

- Aim for ≤ 800 LOC per file. Past that, split on a feature boundary. CI (`check-loc`)
  fails only for a file that is **new in the branch** and over 800; existing files over
  the ceiling are listed as notices and split by issue. There is no committed LOC
  baseline any more (2026-10-01, `docs/decisions.md`): it conflicted on every merge
  and was accepted on every bump.
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
- `settings/store.rs` is the **only** owner of read-modify-write (#491 / ADR-0191):
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

- Git layer returns `Result<T, GitError>` (`crates/kagi-git/src/lib.rs`). Avoid `.unwrap()`
  outside tests.
- User-facing operation errors must surface via the oplog and a bounded toast.
  The oplog owns the complete durable detail; the toast is only a short preview.
  Reserve error modals for cases that require an explicit inspect/acknowledge
  action, or when oplog persistence itself failed.

## Logging rules (important — read before touching any log line)

- The `[kagi] …` lines are a **test contract**. The `KAGI_*` headless harness
  (`src/headless.rs`) greps stderr to verify behavior. Do not change the format,
  wording, or ordering of existing `[kagi]` lines.
- Emit every contract line through the **`klog!`** macro (`crates/kagi-ui-core/src/klog.rs`, ADR-0096):
  `klog!("refreshed")`, `klog!("plan: {} → {}", a, b)` — the `[kagi] ` prefix is
  added by the macro. This is the single, greppable contract channel.
- Use plain `eprintln!`/`tracing` only for ad-hoc human/diagnostic output — never the
  `[kagi]` prefix by hand, and never route a `klog!` contract line through `eprintln!`.
- New features that need headless coverage add `klog!` lines in the established format
  (see `docs/tickets/` T-* specs). Do not "clean up" or reword existing contract lines.

## Adding a new feature

1. Read or write the relevant ADR in `docs/adr/`.
   For decisions too small for an ADR, add one row to `docs/decisions.md`.
2. Git operation? Add the `plan_/preflight_/execute_` triple in the matching
   `crates/kagi-git/src/ops/<feature>.rs` module and a matching integration test in
   `crates/kagi-git/tests/` (#515: backend-only suites live with the crate they
   verify, so `cargo test -p kagi-git` runs them without building the GPUI root;
   root `tests/` keeps only suites that need `kagi::`, the `kagi` binary, shell or
   remote). The shared fixture helpers stay in `tests/support/` and are included
   from `crates/kagi-git/tests/` via `#[path = "../../../tests/support/…"]`.
3. UI? Add `open_/confirm_/start_` methods on `KagiApp`; add the modal in
   `src/ui/modals.rs`.
4. i18n: add EN **and** JA strings to the `Msg` enum in `crates/kagi-ui-core/src/i18n/`.

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
  KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='bottom_panel' \
    cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
  ```

  The feature enables compilation; `KAGI_GUI_E2E=1` permits native execution.
  `KAGI_GUI_E2E_ONLY` is **not optional** — name the scenarios you want, as a
  comma-separated list of substrings. Without it the runner opens a window per
  scenario; an unfiltered run once opened roughly 1,400 windows and crashed
  macOS, so no unfiltered command is written down here to copy.
- **Before committing/pushing, run `cargo fmt --all`.** CI's `fmt + clippy` job is
  advisory (non-blocking) but `cargo fmt --check` exits non-zero on any diff, which
  turns the job red. Run `cargo fmt --check` to confirm clean. Also run
  `cargo clippy --workspace` and don't add *new* warnings (pre-existing v0.2.0 debt
  is tolerated; clippy has no `-D warnings`, so warnings alone won't fail CI, but
  keep your own diff clean — annotate justified cases with `#[allow(...)]`).
- Build + tests passing is necessary but not sufficient for UI behavior. A UI change
  also runs its Tier A scenarios and, for anything that changes what the user sees,
  a Tier B look at the real window (see "Verifying the GUI" below).
- **Repository invariants run through uv, never shell text tools.** The gates live
  in `ci/` as a uv project (`kagi-checks`); run them from the repo root:

  ```
  uv run --project ci check-all              # every gate + the rule selftest
  uv run --project ci check-modal-sections   # one gate
  uv run --project ci check-loc              # new files must stay under 800 LOC (no baseline)
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
- Why: a shared target made Cargo reuse a stale workspace rlib as Fresh (E0432 on a
  new module, 2026-09-07). Each clean worktree build costs 3–4 GB; that is intended.
  When the disk runs low, remove the worktrees of merged PRs first.
- Once a week, when no build is running in that tree, prune artifacts older than
  seven days in every worktree: `cargo sweep --time 7` (requires cargo-sweep).
  Do not routinely clean a worktree target; clean-build benchmarks need an idle
  build window and destroy reusable build artifacts.

## Verifying the GUI

The full recipes live in `.claude/skills/verify/SKILL.md` (Codex reads the same
file through `.agents/skills/kagi-verify`). Read the real file, not a remembered
copy — the skill changes when seams change.

- **Tier A** (`gui_e2e_runner`, ADR-0166) is the gate for UI behavior. Always pass
  `KAGI_GUI_E2E_ONLY`. A scenario that changes settings or the port store restores
  them (`gui_isolation::SavedKeys` / `PortStore`); the runner fails a scenario that
  leaves shared state changed (#899). A change to how outcomes are presented runs
  the scenarios that assert that presentation before merging.
- **Tier B** is the real app driven by `scripts/pidclick.swift` (`CGEventPostToPid`),
  with screenshots from `screencapture -x -o -l<window id>`. It never moves the
  user's pointer or takes the foreground — keep `KAGI_NO_ACTIVATE=1`, a unique
  `USER`, `KAGI_NO_RESTORE=1` and an isolated `KAGI_LOG_DIR`. `cliclick` is banned.
  - A window hidden behind the user's windows stops repainting: if two screenshots
    are byte-identical while `[kagi]` lines show the input arrived, the frame is
    stale. Ask the user to uncover it; do not take the foreground.
  - Clicking the tab strip needs a key window (no `KAGI_NO_ACTIVATE`); ask first.
  - Coordinates are logical points; a screenshot is physical pixels. Convert with
    the window's own scale factor (logical = physical ÷ `backingScaleFactor`, 2 on
    a Retina panel but not on every display), and re-read them from a fresh
    screenshot every time.
- "Computer use" tools are not the GUI driver here: Codex Computer Use cannot reach
  the window server from this environment. Agents that cannot run Tier B say so
  in the PR and leave it to the primary session.
- **Screenshots in a PR**: commit only the images to an orphan
  `pr-assets/<topic>` branch (`git hash-object -w` → `git mktree` →
  `git commit-tree`), push it, and link
  `https://raw.githubusercontent.com/TomiXRM/kagi/<sha>/<file>.png`. Never delete
  those branches — the PR's images point at them.

## Working on a PR: push, review, merge

- **Never rewrite a pushed branch.** No force push, no rebase of a pushed branch:
  take the PR's current base (`origin/main`, or the parent branch of a stacked PR)
  in with a merge. After every push, confirm the remote head
  with `git ls-remote origin <branch>` — a non-fast-forward push is rejected, and
  "pushed" without that check has been wrong.
- **Conflicts**: `CHANGELOG.md`, ADRs and `docs/decisions.md` keep both sides.
  **Code conflicts are resolved by reading each hunk**, never by a mechanical
  "keep both": two PRs adding fields to one struct concatenate into a broken brace
  structure (#900). If `rerere` learned a bad resolution, redo the merge with
  `git -c rerere.enabled=false merge`. Git can also merge cleanly and still drop an
  attribute line (`#[cfg(target_os = "macos")]` before a `mod` in
  `tests/gui_e2e_runner.rs`, #901) — check the runner after a merge.
- **Gate before push**: build, `cargo test --workspace`, the touched Tier A
  scenarios and `uv run --project ci check-all` pass first. Chain them so a failure
  stops the push (`set -e`, or capture the output and test it) — never with `;`.
- **Codex review**: read every Codex line comment before a PR is merged, and reply
  to each one (in Japanese, per the rules below). Fix every P0 and P1, and any P2
  that can lead to a wrong write, data loss or a misleading action. A theoretical case (a race
  with another process replacing the repository, a millisecond window) is closed
  with a reply that gives the reason, and the limit is written into the ADR. Aim
  for one review round per PR; Codex keeps finding the next corner otherwise.
- **Merge** only the head commit Codex reviewed, with CI green and no unanswered
  Codex comment: `gh pr merge N --merge --match-head-commit <sha>`, then confirm the
  PR reads `MERGED`.
- **Stacked PRs** set their base to the parent branch and say "change the base to
  main after #N merges" at the top of the body.
- `Closes #N` only when the PR meets every acceptance criterion of #N; otherwise
  `Refs #N` and list what remains. Split a leftover into its own issue.

## Multi-agent work over herdr

When a PM session drives implementation agents in herdr panes:

- **Send = prompt + Enter.** `herdr agent prompt <pane> '<text>'` places the text;
  `herdr agent send-keys <pane> enter` submits it. Without the second command
  nothing is sent. `pane send-text` never submits. An `agent_prompted` result is
  not proof the agent started — read the pane (`herdr agent read`) or wait for its
  status (`herdr agent wait`).
- **Replies go the same way**: an implementation agent reports to the PM pane with
  `herdr agent prompt <pm pane>` + `send-keys enter`, starting with one tag:
  `[done]` (PR number, remote head SHA checked with `ls-remote`, gates run, and for
  each Codex comment: fixed or answered), `[status]`, `[ask]` (a decision the PM
  owns — stop and wait), `[info]`.
- **One agent, one worktree, one branch.** Never edit another agent's worktree or
  push to a branch another agent owns without saying so first. A PM that merges
  `main` into an agent's PR branch tells that agent before it pushes again.
- **Verify the premise before assigning.** Before writing "reuse the existing X",
  search for X — issue bodies go stale. An agent that finds the premise wrong says
  so instead of building around it.
- **Messages are scoped to the workspace.** A prompt about another repository or an
  issue number that does not match this repository is a misroute: report it, do
  not act on it.
- Watch usage limits in `herdr agent list` (`limit`); an agent near its limit gets
  small, finishable tasks.

## Code Review Rules

### Review language

- Write all user-facing Codex GitHub code-review comments in Japanese.
- Keep code identifiers, commands, file paths, and established technical terms
  in English where that is clearer.
- Write findings, follow-up replies, and approval or no-finding summaries in
  Japanese.
