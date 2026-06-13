# Kagi v0.2.0 — PM Feature & Architecture Inventory

> Phase 0 of the v1.0 re-architecture (`re-architecture` branch).
> Purpose: catalogue everything Kagi v0.2.0 does **before** any redesign, and
> name the structural debt the re-architecture must repay. This is the baseline
> contract: v1.0 must not lose any capability listed under "Features" or any
> guarantee listed under "Safety".

## 0. Snapshot of the codebase

| Metric | Value |
|--------|-------|
| Language / UI | Rust 2021 + GPUI 0.2.2 (`runtime_shaders`) + gpui-component 0.5.1 |
| Source LOC | ~48,900 across 44 `.rs` files |
| Crates | 1 binary (`kagi`) + `xtask` (workspace member); `vendor/gpui-terminal` path dep |
| Largest files | `src/ui/mod.rs` **16,775 LOC**, `src/git/ops.rs` **6,557 LOC** |
| ADRs | 71 (`docs/adr/0001..0071`) |
| Tickets | ~250 (`docs/tickets/`) |
| Tests | 29 integration suites, **306 test fns** (`tests/*.rs`) |
| Prior research | 11 docs in `docs/research/` (gpui-component-audit, zed/jj/gitbutler reuse, conflict-ux ×3, openlogi-learnings, …) |
| Distribution | `xtask` (748 LOC): macOS .app/.dmg, Linux tar.gz, AppImage; GH Actions release |

## 1. Module map (current)

### `src/git/` — git logic (pure-ish, but flat)
| Module | LOC | Responsibility |
|--------|-----|----------------|
| `ops.rs` | 6557 | **Every** write operation as flat `plan_X` / `preflight_X` / `execute_X` fns (checkout, branch CRUD, worktree, stash push/apply/pop, cherry-pick, merge, merge-into-conflict, revert, pull/push/fetch, set-upstream, rename, undo-commit, amend, delete-branch, discard) |
| `resolution.rs` | 2139 | Conflict resolution engine (hunk model, A/B/line accept, assemble result) |
| `conflicts.rs` | 1644 | Conflict detection / session model / continue-abort-skip |
| `message_gen.rs` | 1158 | Smart commit message (rule-based + Ollama LLM opt-in) |
| `staging.rs` | 815 | Stage/unstage, index manipulation |
| `oplog.rs` | 715 | Append-only operation log (`~/.kagi/operations.jsonl`) |
| `diff.rs` / `diffstat.rs` | 635 / 346 | Commit & workdir diffs; per-file +/− counts |
| `checklist.rs` | 475 | Pre-commit checks (conflict markers, secrets, large binaries) |
| `drafts.rs` | 466 | Per-branch commit-message draft autosave |
| `snapshot.rs` | 331 | `RepoSnapshot` build (commits+refs+status+stashes) |
| `log.rs` | 224 | Commit log / revwalk → `Commit`, `CommitId`, `Signature` |
| `message_template.rs` | 230 | `type(scope): summary` + Test/Risk template parse/assemble |
| `status.rs` | 236 | Working-tree status model |
| `refs.rs` | 115 | Branch / RemoteBranch / Tag / Stash / Worktree / UpstreamInfo |
| `trailers.rs` | 111 | Co-author trailer parse |
| `cli.rs` | 109 | `run_git` shell-out fallback (pull/push/fetch network ops) |
| `snapshot`/`message_gen` | — | — |

### `src/graph/` — pure graph layout
| Module | LOC | Responsibility |
|--------|-----|----------------|
| `mod.rs` | 553 | Lane assignment + edge computation (the one genuinely pure, well-tested domain module) |

### `src/ui/` — view + state (heavily coupled)
| Module | LOC | Responsibility |
|--------|-----|----------------|
| `mod.rs` | **16775** | `KagiApp` god-object (~80 fields), ~25 modal structs, all toolbar/status-bar/diff/compare view-models, the 7,300-line `impl KagiApp`, `Render`, conflict-editor handlers |
| `sidebar.rs` | 1356 | Repository navigator (branches/remotes/tags/stashes/worktrees, prefix tree) |
| `theme.rs` | 1173 | 6 color themes + tokens |
| `conflict_view.rs` | 1141 | Conflict dashboard / Conflict Mode view |
| `commands.rs` | 1127 | Command registry + menu bar + overlays |
| `i18n.rs` | 1024 | EN/JA localization table |
| `conflict_editor.rs` | 1017 | 3-pane conflict editor view |
| `branch_menu.rs` | 990 | Branch context menu |
| `inspector.rs` | 794 | Commit inspector (metadata, changed files, diff) |
| `context_menu.rs` | 760 | Commit-row context menu |
| `avatar_fetch.rs` / `avatar.rs` | 713 / 182 | GitHub avatar resolution (ureq) + render |
| `tabs.rs` | 633 | Repo tabs |
| `graph_view.rs` | 552 | Commit graph rendering (lanes/edges/badges) |
| `file_tree.rs` | 453 | Changed-files tree |
| `terminal.rs` | 342 | Integrated terminal (vendored gpui-terminal) |
| `detail_panel.rs` / `commit_list.rs` / `commit_panel.rs` / `diffstat_bar.rs` / `smart_commit.rs` / `watcher.rs` / `assets.rs` | — | leaf views / FS watcher / asset loader |

### `src/main.rs` (1457 LOC)
App shell: window creation, menu actions wiring, `init_tab`, **headless test harness** (`record_headless_op`, `run_headless_discard`, dozens of `KAGI_*` env-var driven flows).

## 2. Feature catalogue (must survive v1.0)

1. **Commit graph** — GitKraken-style lanes, ref badges, HEAD ring, merge nodes, WIP/working-tree row, virtualized (10k+), compact mode, horizontal lane scroll.
2. **Commit inspector** — metadata, GitHub author avatars, co-authors, changed-file tree, syntax-highlighted diffs, resizable split.
3. **Per-file diffstat** — `+N −M` with green/red mini bars in every file list.
4. **Commit suite** — staging, pre-commit checklist (conflict markers/secrets/large binaries), per-branch draft autosave, structured message template (`type(scope): summary` + Test/Risk), amend with SHA-change preview, fixup/squash workflow.
5. **Smart commit messages** — rule-based always-on; **Ollama LLM strictly opt-in** (staged diff only, localhost only, explicit consent).
6. **Dry-run safety** — cherry-pick/revert/checkout conflicts predicted via libgit2 in-memory merges, working tree untouched.
7. **Conflict resolution (Conflict Mode)** — 3-pane editor, file/chunk/**line-level** accept checkboxes, live Result preview/edit, conflict dashboard, Save→stage / Continue flow, abort restores pre-op state, merge-into-conflict, sequencer `--continue` for rebase/cherry-pick/revert.
8. **Backup-then-discard** — discard snapshots blob into ODB + oplog before removing; recoverable.
9. **Branch/tag/stash/worktree management** — checkout (dirty policy), create/rename/delete (safety), set-upstream, merge/rebase direction, worktree creation, stash push/apply/pop, remote-branch tracking checkout.
10. **Pull / push / fetch** — ahead/behind, no force-push (doesn't exist).
11. **Async everything** — all ops off the UI thread, busy indicators, toasts, refresh spinner.
12. **Integrated terminal** — selection, ⌘C/⌘V, theme-matched colors (per-repo PTY sessions).
13. **6 color themes**, **uniform UI zoom**, **EN/JA i18n** (prose localized, Git domain words stay English).
14. **Repo tabs** — multiple repos, stale-while-revalidate cache, async tab load, native menu bar, FS watcher.
15. **Operation log** — append-only `~/.kagi/operations.jsonl` + in-memory ring buffer panel.
16. **Distribution** — macOS .app/.dmg (ad-hoc signed), Linux tar.gz + AppImage, GH Actions release, checksums.

## 3. Safety guarantees (non-negotiable — the product thesis)

| Guarantee | Current mechanism | Where it lives |
|-----------|-------------------|----------------|
| See outcome before any write | Plan modal: current→predicted state, warnings, blockers, recovery recipe; execute button hidden when blockers exist | `ops::plan_*` builds `OperationPlan`; modals in `ui/mod.rs` |
| Destructive commands don't exist | `push --force`, `reset --hard`, `git clean` not implemented anywhere | (absence) |
| Conflicts predicted, not discovered | libgit2 in-memory merge dry-run | `ops::plan_cherry_pick`, `plan_revert`, `plan_merge_branch` |
| Conflicts reversible | Abort restores exact pre-op state; in-progress resolutions autosaved | `conflicts.rs`, `resolution.rs` |
| Nothing silently lost | Stash before checkout; ODB blob backup before discard; append-only oplog with before/after | `ops::plan_discard`, `oplog.rs` |
| Ref moves are last | Working tree written first, refs moved last | per-op in `ops.rs` |
| `plan → confirm → preflight → execute → verify` | Function-level `plan_X`/`preflight_X`/`execute_X` triad per op | `ops.rs` |

## 4. State model (current)

- **No `AppState` entity.** `KagiApp` (in `ui/mod.rs`) is the single gpui `Entity` and holds *everything*: ~80 fields spanning view geometry (split ratios, measured bounds via `Rc<Cell>`), modal visibility (~25 `Option<…Modal>`), git data (rows, branches, tags, stashes, worktrees, upstream), caches (diff/diffstat/tab/avatar), async flags (`busy_op`, generations), conflict-editor state, terminal sessions, tabs.
- **Snapshot-driven**: `reload()` rebuilds derived view data from a fresh `RepoSnapshot`; `reload_external()` for watcher; `detect_conflict_mode()` after every reload.
- **Per-repo data is duplicated** between `KagiApp` top-level fields (active tab) and `tab_cache: HashMap<PathBuf, TabViewState>` (inactive tabs) — a fragile split.

## 5. Async / concurrency model (current)

- `cx.background_spawn` (24×) + `cx.spawn` (24×) directly inside `ui/mod.rs`.
- **No `GitWorker` / dedicated repo thread** (architecture.md specified one; never built). Each op opens its own `Repository` on a background thread.
- Network ops (pull/push/fetch) shell out via `cli::run_git` rather than libgit2.
- Race control via monotonic generation counters: `watcher_generation`, `switch_generation`, `modal_replan_gen`, `draft_save_gen`, `next_toast_id`.

## 6. Test & QA surface (current)

- 29 integration suites (`tests/*.rs`), 306 test fns, all against tempdir fixture repos (`scripts/make_fixture.sh`); **never against real repos**.
- Coverage skews to git logic: ops, amend, branch ops, pull/push, stash, discard, revert, undo, staging, diff/diffstat, conflicts, drafts, checklist, message gen/template, trailers, snapshot, graph_layout, i18n, qa_audit.
- **UI is barely tested in-process**: a few `#[gpui::test]` in `ui/mod.rs` (toolbar_tests, conflict_editor_geometry_tests). The real UI test mechanism is the **`KAGI_*` headless env-var harness** in `main.rs` (47 vars: `KAGI_AUTO_CONFIRM`, `KAGI_CHECKOUT`, `KAGI_DISCARD`, …) driving the live app, which is brittle and lives in the binary.

## 7. Structural debt (what the re-architecture must repay)

### 7.1 God-files / mixed responsibilities
- `ui/mod.rs` (16.7k LOC) fuses **state + every view-model + all modal logic + all operation orchestration + conflict editor + Render** into one struct + one 7,300-line `impl`. Unnavigable, unreviewable, merge-conflict magnet.
- `ops.rs` (6.5k LOC) is one flat namespace of ~60 free functions; no per-operation modules, no shared `Operation` trait, lots of copy-paste across the plan/preflight/execute triads.

### 7.2 UI ↔ Git tight coupling (the headline problem)
- `ui/mod.rs` calls `Repository::open` **80×** and uses `git2::` **81×** *inline* — i.e. raw libgit2 logic lives in the view layer, bypassing `ops.rs` entirely.
- Only **15** references to `ops::` from the UI. The "GitBackend trait" in architecture.md does not exist; there is no enforced boundary preventing the UI from running git2 directly.
- Consequence: the safety pipeline is **not** structurally enforced — the UI *can* (and does) touch the repo outside `plan→…→verify`.

### 7.3 State management fragility
- ~80-field god-struct with no sub-state grouping; active-tab vs `tab_cache` duplication; geometry smuggled through `Rc<Cell<(f32,f32)>>` written at paint time; ad-hoc generation counters instead of a structured task/cancellation model.

### 7.4 Async without a backend
- No worker abstraction; every background closure re-opens the repo and calls git2 directly. Cancellation is by hand-rolled generation comparison. Hard to test, easy to race.

### 7.5 Testability
- Pure domain (graph, message gen, templates) is well-tested. But **UI behavior is only testable via a 47-var env harness baked into `main.rs`** that drives the real binary — no headless view-model layer to unit-test. View-models are entangled in `KagiApp`, so they can't be exercised without a window.

### 7.6 Forced / ad-hoc implementations to revisit
- Geometry via `Rc<Cell>` written from a paint-time canvas (inspector_geom, conflict_geom, conflict_ab_geom) — works, but a smell of missing layout primitives.
- Avatar HTTP via `ureq` because gpui 0.2.2 ships only the http-client trait (ADR-0037) — keep, but isolate.
- Vendored `gpui-terminal` fork for selection/copy (ADR-0035) — keep, isolate behind a port.
- Network git via CLI shell-out (`cli.rs`) instead of libgit2 — intentional, document as a backend adapter.
- `KAGI_*` env harness in `main.rs` — replace with a real testable command/view-model layer.

## 8. What is already good (keep / build on)

- The **safety philosophy is genuinely implemented** at the function level (`plan_/preflight_/execute_` triads, in-memory dry-runs, oplog, blob backups, no destructive commands). v1.0 must *formalize* this into a typed pipeline, not reinvent it.
- `graph::mod.rs` is a clean, pure, well-tested domain module — the template for the target Domain layer.
- The **ADR discipline** (71 ADRs) and ticket board capture product intent precisely; the re-architecture must keep this rigor (new ADRs for every structural decision).
- Snapshot-based rendering is a sound model — keep it; just move it behind a backend boundary.

## 9. Target boundaries (preview — detailed in architecture.md after research)

`domain` (pure: models, graph layout, plan types) → `git-backend` (trait + git2/CLI adapters, snapshot, all `plan/preflight/execute/verify`) → `app` (AppState, OperationController, async task/cancellation, persistence, oplog) → `ui` (view-models + GPUI views + commands/actions). The UI must **never** open a `Repository` or call `git2::` directly again — that is the single most important invariant of v1.0.
