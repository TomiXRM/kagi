# Architecture Cleanup Roadmap (2026-06-21)

> Post-sprint status + concrete plan for the remaining architecture work.
> Based on the codebase review + 6 PRs already merged (#55-#58 + skill).

## Current state (what's done)

| Area | Status | PR |
|---|---|---|
| Safety pipeline (Backend::run) | **Done** — preflight enforced for all 30 mutating paths | #55 |
| RepoSession (read-path) | **Done** — 42 read-path sites use session.backend() | #55 |
| RepoWorker (write-path infra) | **Infra done** — worker thread exists; 32 callers NOT migrated | #58 |
| ToastStack separation | **Done** — Rc<RefCell>, logic extracted, 3 unit tests | #57 |
| Highlight async | **Done** — text-first render, background highlight swap | #56 |
| Dead code sweep | **Done** — -416 LOC | #55 |
| diff cache + reload_external off-thread | **Done** | #55 |

## Current pain points (what's NOT done)

### 1. God-files (unchanged from review)

| File | LOC | Problem |
|---|---|---|
| `src/ui/mod.rs` | 6406 | KagiApp struct (105 fields) + 2 impl blocks + free functions + bootstrap |
| `src/ui/render.rs` | 6100 | render() = 848 LOC + 77 cx.notify + per-frame clones |
| `src/ui/modals.rs` | 3776 | 23 modal renderers, 55 cx.notify |
| `src/git/ops/pull_push.rs` | 2077 | pull+push+fetch in one file |
| `src/ui/commands.rs` | 1748 | command dispatch + menu state |
| `src/headless.rs` | 1713 | 48 KAGI_* hooks, 26 raw git2 opens |

### 2. cx.notify() = 329 sites → full repaint every time

The root cause of all render-path performance issues. Every notify on the root
`Entity<KagiApp>` repaints the entire 848-line render tree.

### 3. KagiApp = 105-field god-struct

Every feature added 3-8 fields. No isolation between concerns.

### 4. No crate separation

`src/ui`, `src/git`, `src/headless` all in one binary crate. The "no git2 in UI"
invariant is a CI grep, not a compile error.

---

## Roadmap: 5 phases, ordered by value/effort

### Phase A — Worker thread caller migration (Medium, high value)

**Goal:** eliminate 32 `Backend::open` re-opens on write paths.

Each `*_blocking` fn migrates from:
```rust
let mut repo = Backend::open(&repo_path)?;
repo.run(&op, &plan)?
```
to:
```rust
let rx = session.submit(op, plan)?;
let result = rx.recv()?;
```

**Steps:**
1. `stash_push_blocking` / `stash_pop_blocking` / `stash_drop_blocking`
2. `checkout_blocking` / `checkout_tracking_blocking`
3. `merge_blocking` / `cherry_pick_blocking` / `revert_blocking`
4. `commit_blocking` / `amend_blocking` / `undo_blocking`
5. `pull_blocking` / `push_blocking` / `switch_to_latest_blocking`
6. `discard_blocking` / `delete_branch_blocking`
7. `branch_plan_blocking` / `set_upstream_blocking` / `rename_branch_blocking`
8. `create_worktree_blocking`

Each step = 1 commit, tests green. The `session` is already on KagiApp; the
`*_blocking` fns need it passed in (currently they take `repo_path: &Path`).

**Risk:** Low — mechanical swap. The worker already passes all tests.
**Effort:** 1-2 PRs (8 sub-commits each).

---

### Phase B — Render-path clone elimination (Small, immediate UX win)

**Goal:** kill per-frame Vec/struct clones in render().

**Steps:**
1. `MainDiffView` / `compare_view` / `conflict` → `Arc<MainDiffView>` etc.
2. `CommitRow` clone in `render_rows` → borrow `&rows[ix]`
3. `avatar_color` / `avatar_initial` → pre-compute on CommitRow at build time
4. `theme()` / `scaled_px()` → hoist to one local per render()
5. `row.edges.clone()` in graph_canvas → borrow `&[GraphEdge]`

**Risk:** Low — pure perf, no behavior change.
**Effort:** 1 PR.

---

### Phase C — Entity decomposition (Large, root cause fix)

**Goal:** break KagiApp into child entities so cx.notify is scoped.

**Priority order** (by notify count + self-containment):

| Component | notify | Self-contained? | Approach |
|---|---|---|---|
| **Toasts** | done (Rc<RefCell>) | yes | → Entity (Phase C.0) |
| **CommitPanel** | 14 | yes (own state + input) | Entity<CommitPanel> |
| **ConflictEditor** | 12 | yes (3-pane editor) | Entity<ConflictEditor> |
| **Sidebar** | 14 | yes (branch list + filter) | Entity<Sidebar> |
| **OpLogPanel** | low | yes (ring buffer) | Entity<OpLogPanel> |
| **Inspector** | low | yes (commit detail) | Entity<Inspector> |
| **FileHistory** | low | yes (per-file log) | Entity<FileHistoryView> |
| **Terminal** | done (Entity<TerminalView>) | yes | — |

**Pattern (proven by Terminal + ThemeSelect):**
```rust
// Before: flat field on KagiApp
pub conflict: Option<ConflictMode>,

// After: child Entity
pub conflict: Option<Entity<ConflictEditor>>,
```

Each Entity owns its state + render + cx.notify scope. KagiApp's render()
reads `self.conflict.read(cx)` or delegates via `.update(cx, ...)`.

**Risk:** Medium — borrow checker friction around `&mut self` + child entity
updates. Mitigated by the existing Terminal/ThemeSelect patterns.
**Effort:** 1 PR per component (8 PRs total).

---

### Phase D — File decomposition (Medium, maintainability)

**Goal:** split god-files below 800 LOC (AGENTS.md target).

| File | Split into |
|---|---|
| `mod.rs` 6406 | `mod.rs` (struct + bootstrap) + `state.rs` (field init) + `actions.rs` (command dispatch) |
| `render.rs` 6100 | `render/mod.rs` (dispatch) + `render/commit_list.rs` + `render/inspector.rs` + `render/sidebar.rs` + `render/toasts.rs` + `render/diff.rs` |
| `modals.rs` 3776 | `modals/mod.rs` (ActiveModal + dispatch) + `modals/checkout.rs` + `modals/branch.rs` + `modals/stash.rs` + `modals/discard.rs` + shared `modal_card.rs` |
| `pull_push.rs` 2077 | `ops/pull.rs` + `ops/push.rs` + `ops/fetch.rs` |
| `commands.rs` 1748 | `commands/mod.rs` + `commands/menu_state.rs` + `commands/dispatch.rs` |
| `headless.rs` 1713 | retire 30 redundant hooks (ADR-0077); keep ~10 UI-state hooks |

**Risk:** Low — mechanical file moves, no behavior change.
**Effort:** 1 PR per god-file (6 PRs).

---

### Phase E — Crate separation (Large, compile-time enforcement)

**Goal:** `kagi-ui` crate has no `git2` dependency (compile error, not CI grep).

```
kagi (bin)
  ├─ kagi-ui          Entity trees, Render, components (NO git2)
  │   └─ kagi-app     AppState, RepoSession, OperationController
  │       └─ kagi-git GitBackend + RepoWorker + ops
  │           └─ kagi-domain  pure: models, graph, diff, conflict FSM
  └─ xtask
```

**Prerequisite:** Phase C (Entity decomposition) must be partially done so
the UI crate boundary is clean (UI entities don't touch Backend directly).

**Risk:** Medium — workspace restructuring, import path churn.
**Effort:** 1 large PR.

---

## Recommended order

```
Phase B (render clone)     ← 1 PR, immediate win, do first
Phase A (worker migration) ← 2 PRs, finishes the RepoWorker story
Phase C.1-C.3              ← CommitPanel + ConflictEditor + Sidebar
Phase D (file splits)      ← parallel with C, mechanical
Phase C.4-C.8              ← remaining entities
Phase E (crate split)      ← last, needs C partially done
```

**Total:** ~15 PRs over several sessions. Each is independently mergeable.
