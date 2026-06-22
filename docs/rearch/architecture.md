# Kagi v1.0 — Target Architecture

> Phase 2 of the re-architecture (`re-architecture` branch). Synthesizes the 10
> research docs in `docs/rearch/research/` and `docs/rearch/inventory.md` into the
> structure v1.0 is built on. Every structural decision here has (or gets) an ADR
> in `docs/adr/` (0072+). Implementation (Phase 3) follows this doc; if a change is
> needed, **update this doc / the ADR first, then the code**.

## 0. The one invariant

> **The UI layer must never open a `git2::Repository` or call `git2::` directly.**

Today this is violated ~80× in `ui/mod.rs`. v1.0 makes it a **compile error**: `git2`
is not a dependency of the `kagi-ui` crate. All Git work flows through the
`plan → confirm → preflight → execute → verify → log` pipeline. This is the
product thesis (safety-first, predict-before-act) expressed as a type boundary.

## 1. Crate topology (the dependency DAG)

A Cargo workspace replaces the single `kagi` binary. Dependencies point **down only**:

```
                 ┌─────────────────────────────────────────────┐
   kagi (bin)    │ window/shell/menu wiring, main(), shell only │
                 └───────┬───────────────┬───────────────┬──────┘
                         │               │               │
                  ┌──────▼──────┐  ┌─────▼──────┐  ┌─────▼──────┐
                  │  kagi-ui    │  │  kagi-app  │  │  (xtask)   │
                  │ views, VMs, │→ │ AppState,  │  │ packaging  │
                  │ components, │  │ Sessions,  │  └────────────┘
                  │ theme,i18n, │  │ Operation- │
                  │ commands    │  │ Controller │
                  └──────┬──────┘  └─────┬──────┘
                         │   ┌───────────┘
                         │   │
                  ┌──────▼───▼──────┐        ┌──────────────────────┐
                  │   kagi-domain   │◄───────┤      kagi-git         │
                  │ pure: models,   │        │ GitBackend trait,     │
                  │ graph, diff,    │        │ git2 + CLI adapters,  │
                  │ conflict FSM,   │        │ snapshot, oplog,      │
                  │ plan types,     │        │ worker thread (owns   │
                  │ rules, settings │        │ git2::Repository)     │
                  └─────────────────┘        └──────────┬───────────┘
                                                        │
                                                   git2, portable-pty (none in ui)
```

| Crate | Depends on | May import `git2`? | May import `gpui`? |
|-------|-----------|:---:|:---:|
| `kagi-domain` | — | ❌ | ❌ |
| `kagi-git` | domain, **git2** | ✅ | ❌ |
| `kagi-app` | domain, kagi-git, gpui | ❌ (only via kagi-git) | ✅ |
| `kagi-ui` | domain, kagi-app, gpui, gpui-component | **❌ (enforced)** | ✅ |
| `kagi` (bin) | all of the above | ❌ | ✅ |
| `kagi-test-fixtures` | git2 (dev) | ✅ | ❌ |
| `xtask` | — | ❌ | ❌ |

**Enforcement**: `kagi-ui/Cargo.toml` simply does not list `git2`/`kagi-git`. A CI
grep gate (`! grep -r 'git2::' crates/kagi-ui/src`) is a belt-and-suspenders backstop.
The `Operation` request types the UI needs live in `kagi-domain`, so the UI can
*describe* work without depending on the layer that *does* it.

> Decision recorded in **ADR-0072** (workspace crate split + git2 confinement).
> The crate boundary is the mechanism; a module boundary inside one crate was
> rejected because it can't make the leak a compile error.

## 2. Layer responsibilities

### 2.1 `kagi-domain` — pure Rust, no I/O, no frameworks
The home of everything unit-testable without a window or a repo. Migrated from the
already-pure parts of the git backend (now `crates/kagi-git/`, extracted in ADR-0115;
formerly `src/git/`) and `src/graph/`:

- **Models**: `CommitId`, `Commit`, `Signature`, `Branch`/`RemoteBranch`/`Tag`/`Stash`/`Worktree`, `Head`, `WorkingTreeStatus`/`FileStatus`/`ChangeKind`, `RepoSnapshot`.
- **Graph layout** (`graph::layout`): the gitk-style lane/edge algorithm — already pure (`src/graph/mod.rs`), moved verbatim with its 12 tests + `check_invariants`. Output `GraphLayout`/`GraphRow`/`GraphEdge` stays free of display concerns.
- **Diff model**: `Diff`/`FileDiff`/`Hunk`/`DiffLine`, `FileDiffStat`, `bar_segments`, and `build_file_tree` (lifted out of `ui/file_tree.rs`, decoupled from `SharedString`).
- **Conflict domain**: the hunk model + line/chunk/file resolution + `assemble()` from `resolution.rs` (2139 LOC, already pure), plus the **conflict session as a pure FSM** (`SessionState` with derived `can_continue/can_abort/can_skip`) replacing the duplicated `can_continue` logic.
- **Plan types**: `OperationPlan`, `StateSummary`, `Warning`, `Blocker`, `Recovery`, `RepoFingerprint` — and the `Operation` request enum (what the UI builds, what the backend executes).
- **Rules**: `message_template` parse/assemble, `checklist` rules, `message_gen::rule_based`, `trailers` parse — all pure today, moved as-is. Unify the scattered checklist logic into one `evaluate_commit` verdict.
- **Settings model**: a typed `Settings` struct (theme, lang, zoom, compact, panel sizes…) and the `i18n` key enum (`Msg`).

### 2.2 `kagi-git` — the Git backend (the only place git2 lives)
- **`trait GitBackend: Send`** — the sole interface the app sees. Reads (`snapshot`, `diff_commit`, `diff_workdir`) + the operation pipeline primitives. Async-returning (results delivered off the worker thread).
- **Unified `Operation`** dispatch collapses the ~30 `plan_X`/`preflight_X`/`execute_X` triads from `ops.rs` into one shape: `plan(&Operation) -> OperationPlan`, `preflight(&Operation, &OperationPlan)`, `execute(&Operation) -> OperationOutcome`, `verify(&Operation, &OperationOutcome)`. Per-operation logic moves into per-op modules (`ops/checkout.rs`, `ops/cherry_pick.rs`, …) instead of one 6.5k-LOC file. Shared helpers (dirty-WT formatter, standard blocker set) are written once.
- **Two adapters**: `git2` adapter (default; the only one that does in-memory dry-runs — the reason we keep libgit2 over pure CLI), and a **CLI adapter** for network ops (`fetch`/`pull`/`push` via `run_git`, prompts-off, timeout) — already the right call (`cli.rs`), now formalized.
- **Worker thread**: `git2::Repository` is `Send` but `!Sync`, so each `RepoSession` owns **one worker thread** that holds the `Repository` and serializes operations via a channel. This kills the "re-open the repo in every background closure" pattern (80× today).
- **Dry-run safety**: in-memory `cherrypick_commit`/`merge_trees` → predicted file set + conflict flag, working tree untouched. Preserved verbatim.
- **Oplog**: append-only `~/.kagi/operations.jsonl`, widened to record SHAs + recovery handles (today it only stores stringified summaries). Blob-backup-before-discard preserved.

> ADR-0073 (GitBackend trait + Operation enum + worker thread), ADR-0074 (oplog format v2).

### 2.3 `kagi-app` — state, the pipeline controller, async, persistence
- **`AppState`** (single `gpui::Entity`) owns: the `Workspace` (list of `RepoSession`s + active index), global services (settings, theme, i18n active locale), and cross-cutting UI shell state (which panels are visible, command palette).
- **`RepoSession`** — *one per tab, fully self-contained* (ADR-0075). Replaces the active-fields-vs-`tab_cache` duplication. Each session holds: its `GitBackend` handle + worker, the latest `RepoSnapshot` + derived view data, **`Selection { Wip, Commit(CommitId) }`**, `RepoMode { Normal, Conflict(ConflictSession) }`, its `DiffService` cache, its terminal session, its FS watcher, and a `Freshness { Loading, Fresh, Stale }`. Switching tabs is a **zero-frame `active = idx` swap** — no build/apply bridge, transient state (selection, scroll) preserved per tab.
- **`OperationController`** — enforces the pipeline **once**, centrally: `request(Operation)` → build plan (off-thread) → return plan for confirm → on confirm, preflight (re-snapshot; abort if the repo changed) → execute → verify → append oplog → refresh snapshot. Owns cancellation (replacing the ad-hoc `busy_op` + generation counters with a structured task handle). This is the *only* path that mutates a repo.
- **Async model**: app-layer tasks via `cx.background_spawn`, but git work is delegated to the session's worker thread; the controller awaits results and applies them on the foreground. Generation/supersede logic (rapid tab switches, debounced re-plan) lives here, structured, not sprinkled through the view.
- **Services**: `DiffService` (commit-identity `DiffKey` cache + background syntax highlight), `SettingsService` (typed read/write-through to `~/.kagi/settings.json`, serde behind a boundary, foreign-key-preserving), avatar resolution, smart-commit (rule-based sync + Ollama async, opt-in/localhost/staged-diff-only invariants intact).

### 2.4 `kagi-ui` — view-models, views, components (no git2)
- **View-models** (plain data, unit-testable, no `git2`, minimal gpui): `CommitGraphVM`, `InspectorVM`, `DiffVM`, `CommitDraftVM` (collapses 10+ flat commit fields; `InputState` synced *from* the VM, killing the `pending_smart_msg` hack), `ConflictVM`, `SidebarVM`, `StatusBarVM`/`ToolbarVM`. The VM is built from `RepoSession` data; the view renders the VM and emits **intents** (e.g. `Select(CommitId)`, `RequestOperation(Operation)`) back to the app.
- **Views**: GPUI `Render` impls, one self-contained component per feature area (graph, inspector/diff, sidebar, commit panel, conflict dashboard + 3-pane editor, terminal, bottom panel, tab strip). No more single 16.7k-LOC god-view.
- **Modals**: one `enum ActiveModal` per session (carrying each modal's lazy `InputState`/focus) replaces the ~25 `Option<…Modal>` fields.
- **Components**: a thin Kagi layer over `gpui-component` (Input, Icon, Tooltip, Root, Scrollbar, Checkbox, Dialog, toast) — adopt incrementally per the gpui-component audit. Keep `uniform_list` virtualization (proven for 10k+); add `gpui_component::Scrollbar`.
- **Theme / i18n / zoom**: keep the custom semantic-token `Theme` + the one-way `sync_gpui_component_theme` bridge (Kagi stays single source of truth — gpui-component's ThemeProvider can't model lanes/terminal/avatar). `Msg::t` lookup. Uniform zoom via `set_rem_size(16*zoom)` + `scaled_px` helpers.

### 2.5 `kagi` (bin) — the shell
Window creation, native menu bar wiring, `Action` dispatch, app bootstrap, and a
**thin** headless entry (see Testing). No business logic.

## 3. Commands / actions
Keep `commands.rs`'s command registry (the cleanest existing UI subsystem, ADR-0029):
a table of gpui `Action`s, "disabled = handler unregistered", `os_action` for
Edit/clipboard. Move it to the app/ui boundary, drive the menu bar from it, and add a
**command palette** (cmd-shift-p) off the same table. Every operation is reachable as
a command → menu item → keybinding uniformly.

## 4. Persistence
- `~/.kagi/settings.json` — typed `Settings` via `SettingsService` (write-through, atomic, preserves unknown keys). Env vars (`KAGI_THEME`/`KAGI_LANG`/`KAGI_LOG_DIR`) override but don't persist.
- `~/.kagi/operations.jsonl` — append-only oplog v2 (SHAs + recovery).
- `~/.kagi/avatars/` — avatar disk cache.
- `~/.kagi/drafts/` — per-(repo,branch) commit-message drafts.
- Workspace session list (open tab paths + active index + per-tab selection by `CommitId`) — restored on launch.

## 5. Testing strategy (4-layer pyramid)
Mirrors the crate split (ADR-0077):

1. **domain** — plain `#[test]` unit tests (graph layout, diff/diffstat, conflict FSM + assemble, template/checklist/message-gen, settings). The bulk of tests live here; no fixtures, no window.
2. **kagi-git** — fixture-integration tests against tempdir repos via the new **`kagi-test-fixtures`** crate (replaces the copy-pasted inline `git()` builders across 29 suites; `make_fixture.sh` logic ported into Rust). All ~306 existing op/staging/conflict/etc. tests move here, re-pointed at the backend API.
3. **kagi-app** — view-model + `OperationController` tests: mostly plain unit tests (VMs are plain data) + targeted `gpui::test` / `TestAppContext` for async/controller flows.
4. **kagi-ui** — `gpui::test` + `VisualTestContext` for render/input on the components that genuinely need a window.

**Retire the `KAGI_*` harness**: the 47-var env harness baked into `main.rs` exists only because the app/view layer was untestable. With VMs and the controller testable directly, delete the duplicated plan/execute env paths; keep at most a thin `xtask e2e` JSON-output CLI for smoke tests. Add **`ci.yml`** (fmt + clippy + `cargo test --workspace`, macOS + Linux; network pull/push excluded). Today there is **no test CI** — only the release workflow.

## 6. Packaging
`xtask` (macOS .app/.dmg, Linux tar.gz, AppImage) is healthy and unchanged in
behavior; only its `-p` / target paths follow the workspace re-split (the bin crate
is now `crates/kagi`). The GH Actions release workflow stays; `ci.yml` is added
alongside it. ADR-0038/0047 remain in force.

## 7. Migration strategy — strangler, not big-bang
The 49k-LOC app cannot be rewritten safely in one shot. Order (each step keeps
`cargo test --workspace` green; details in `docs/rearch/migration/`):

1. **Carve the workspace skeleton**: create `crates/{kagi-domain,kagi-git,kagi-app,kagi-ui,kagi}` + `kagi-test-fixtures`; move the bin in. Get it compiling with code still in its current shape (re-exports/`pub use` bridges).
2. **Extract `kagi-domain`**: move the already-pure modules (graph, diff/diffstat models, resolution, templates, checklist, message-gen rules, status/refs models, plan types). Re-point the 306 tests. *This is low-risk and unlocks most of the test pyramid immediately.*
3. **Extract `kagi-git`**: introduce `GitBackend` trait + `Operation` enum; move `ops.rs` per-op into modules behind it; stand up the worker thread; keep the git2 adapter behavior-identical (move verbatim, refactor second — lean on the tests).
4. **De-leak the UI**: route every `Repository::open`/`git2::` site in the view through `OperationController`/`GitBackend`. Add the CI grep gate. This is the highest-effort step (80 sites) — a strong candidate to delegate to Codex (GPT-5.5 high/xhigh) in well-scoped batches per feature area.
5. **Introduce `kagi-app`**: `AppState`/`RepoSession`/`OperationController`; collapse active-vs-cache; selection enum; `RepoMode`.
6. **Split the view**: carve `ui/mod.rs` into per-feature components + view-models; collapse modals into `ActiveModal`.
7. **Retire `KAGI_*`**, add `ci.yml`, update README for v1.0.

Each feature area (graph, conflict, diff, staging, tabs, terminal, theme/i18n) migrates
behind its research doc + ADR. The safety pipeline and every v0.2.0 feature in
`inventory.md` §2–3 must remain green at every step.

## 8. Open questions to resolve during implementation
(Carried from the research docs; each gets an ADR or a migration note when decided.)
- `Operation` dispatch: enum vs trait-object (research #3 leans enum for exhaustiveness).
- Whether `kagi-app` can stay fully git2-free given the worker thread (yes if the worker lives in `kagi-git` and app holds only a handle — current plan).
- Color stability for lanes (stable branch-lineage key vs `index % 6`) — MVP keeps index.
- Watcher: per-active-repo vs per-session (Stale badges) — research #7 open.
- Settings: serde-typed vs KV; per-frame theme accessor stays lock-free (atomic mirror).
- Conflict autosave debounce (ADR-0057 says 250ms; today eager) — fix during conflict migration.
- gpui 0.2.2 exact `gpui::test`/`VisualTestContext` surface — needs a 1-shot PoC before relying on it for ui tests.
