# Kagi Performance Review

> Focused performance document for the Kagi GPUI Git client. Derived from the
> performance pass of `/docs/codebase-review.md` §3, expanded with strategy
> sections. Every claim grounded in `file:line` evidence; all headline numbers
> verified against the tree.
>
> **Root cause up front:** ~90% of the findings below trace back to one
> architectural fact — **`KagiApp` is a single 104-field `Entity<T>` and
> `cx.notify()` (329 call sites) repaints the entire 848-line `render()` tree.**
> The single highest-leverage change is decomposing `KagiApp` into child
> entities (refactor Step 5.1). Until then, the tactical fixes in §1–§7
> materially help but do not cure the root cause.

---

## 0. Measured baseline (what the code does today)

| Metric | Value | Implication |
|---|---|---|
| `cx.notify()` call sites in `src/` | **329** | Every UI change repaints the whole app |
| `Backend::open` call sites in `src/ui/` | **94** (132 total) | Repo re-opened per interaction, ~1–5 ms each |
| `render()` function length | **848 LOC** | Longest render path; runs per notify |
| Files > 800 LOC | **29** | Navigation + compile cost |
| `git2::statuses` pathspec | **none** (full recursive scan) | Dominant snapshot cost on messy repos |
| Per-file diff content cache | **none** (file-list only) | Click-back recomputes the diff |
| Tree-sitter highlight | **synchronous, on UI thread** | Large-diff open stalls |
| Graph layout | **full recompute per reload** | O(commits × lanes) per refresh |
| Graph paint | **per-edge PathBuilder per row per frame** | Dominant per-frame cost on wide repos |
| Watcher poll interval | **100 ms** (should block on recv) | Constant wake-ups |
| Auto-fetch ticker | **per-tab**, armed from `render()` | N tabs = N parallel fetches |

---

## 1. Likely expensive operations (ranked by user-visible impact)

### 1.1 Full `git2::statuses` scan with no pathspec — CRITICAL
**Evidence:** `src/git/status.rs:51-60` — `StatusOptions::new()
.include_untracked(true).recurse_untracked_dirs(true).renames_head_to_index(true)`,
no pathspec, called from `snapshot()` (`src/git/snapshot.rs:77`).

**Cost:** Walks every file in the workdir on every reload. The
`is_nested_git_dir` skip (`status.rs:152,166-170`) only helps for dirs
containing `.git`, not arbitrary large untracked trees (`node_modules`, a
sibling `.claude/worktrees/`, a build `target/`).

**Impact:** A 100k-file untracked tree turns every external git event into a
multi-second freeze (compounded by §2.1 — it runs on the UI thread).

**Fix:**
1. Run status off-thread (§2.1).
2. When the watcher reports a sub-path change, scope the pathspec.
3. Cache the status result; re-scan only on worktree-classified events.

### 1.2 Graph layout full recompute on every reload — HIGH
**Evidence:** `src/ui/commit_list.rs:189 let graph = layout(&snap.commits);`
inside `build_commit_rows`, called from `build_tab_view` on every `reload()`
(`src/ui/mod.rs:1645`). `layout` is O(commits × active_lanes) with three
linear scans per commit (`crates/kagi-domain/src/graph.rs:137,176,286,296`).

**Cost:** 10k-commit repo → ~50k comparisons, recomputed after every external
change and every op. No incremental path — one new commit re-layouts the whole
window.

**Impact:** Combined with §2.1, any HEAD movement in a sibling worktree
triggers full snapshot + full layout on the UI thread.

### 1.3 `graph_ahead_behind` for every local branch — MEDIUM
**Evidence:** `src/git/snapshot.rs:126-151` — for each local branch with an
upstream, `repo.graph_ahead_behind(target_oid, up_oid)`. No filter on the
current branch. Each call is an O(commits) graph walk.

**Cost:** 50 local branches → 50 graph walks per snapshot. Worktree WIP
refresh opens each linked worktree and runs `working_tree_status` again
(`snapshot.rs:373-381`).

**Impact:** Multi-second snapshots on branch-heavy repos; the sidebar only
shows ahead/behind for visible branches.

### 1.4 Per-file diff content recomputed on every open — HIGH
**Evidence:** `src/ui/mod.rs:770 diff_cache: HashMap<usize, Option<Vec<FileStatus>>>`
holds only the file *list*. `set_commit_main_diff`/`open_main_diff`
(`src/ui/mod.rs:3523-3533`) calls `repo.commit_file_diff(&id, &path)` every
time.

**Cost:** Click A → F (diff). Click B. Click back to A → F: full tree-diff +
hunk extraction again.

### 1.5 Synchronous tree-sitter highlighting on UI thread — HIGH
**Evidence:** `src/ui/mod.rs:3291 let _ = highlight_diff_rows(&mut rows,
&path);` inline. `src/ui/diff_view.rs:194-285` builds a `SyntaxHighlighter`,
feeds a Rope, parses + styles the entire diffed file content.

**Cost:** 5k-line file → multi-ms parse on the UI thread per open (compounds
§1.4 — re-run on every click-back).

### 1.6 Empty-dir pruning in discard walks up the tree — LOW
**Evidence:** `src/git/ops/discard.rs:259-267` — `remove_dir` on each empty
parent. No `.gitignore` consultation.

**Cost:** Usually trivial; pathological on deep ignored trees.

---

## 2. UI-thread blocking risks

### 2.1 `reload_external` runs the full snapshot on the UI thread — CRITICAL
**Evidence:** `src/ui/mod.rs:1788` `reload_external` → `:1798 self.reload()` →
`:1620-1627` `Backend::open` + `repo.snapshot(10_000)` inline. Contrast
`refresh_working_tree_external` (`:1826-1836`) which correctly uses
`cx.background_spawn`.

**Cost:** A `WatchEvent::Git` (HEAD/refs change from a terminal, sibling
worktree, or auto-fetch) drives a full re-snapshot synchronously inside the
UI update closure.

**Impact:** Every external `git pull`/`fetch`/commit/checkout stalls the UI
frame for the duration of a snapshot — hundreds of ms on a 10k-commit repo.

**Fix:** Mirror `refresh_working_tree_external`: `cx.background_spawn` the
`snapshot()`, apply on the UI thread via `apply_tab_view`. (Refactor Step 3.1.)

### 2.2 Synchronous git2 inside `render()` — CRITICAL
**Evidence:**
- `src/ui/render.rs:446-450`: `if let Ok(backend) = Backend::open(&repo_path) {
  self.seed_history_from_reflog(&backend); }` inside `Render::render`.
- `src/ui/mod.rs:2059` (`ensure_avatars`, from `render()` `:423`):
  `avatar_fetch::repo_github_coords(&repo_path)` — read-only git2 on UI thread.
- `src/ui/mod.rs:1886-1906` (`detect_conflict_mode`, from `render()` `:429`):
  opens repo + `detect_conflict_session` synchronously.

**Cost:** Guarded "once per repo," but the first git2 open runs on the render
thread. On a slow/network filesystem, `Backend::open` is 100 ms+.

**Impact:** Visible first-frame stall after tab switch on slow disks.

**Fix:** Spawn these one-shot detection passes via `cx.background_spawn` +
`cx.spawn` marshalling (the pattern already used for `file_history` at
`src/ui/mod.rs:3452-3493`). The guarded boolean stays; the work moves
off-thread.

### 2.3 Filesystem I/O in `cx.spawn` (foreground executor) — HIGH
**Evidence:** `src/ui/operations/commit.rs:140-158` `schedule_draft_save`:
`cx.spawn(async move |this, acx| { Timer::after(250ms).await;
this.update(acx, |app, _cx| { … kagi::git::save_draft(&rp, &branch, &msg,
&mode); }); })`. `save_draft` does real file I/O on the UI-thread executor.
Same at `:887` `clear_draft`.

**Cost:** Blocking I/O stalls the next frame. The 250 ms debounce masks it
during typing but a slow disk surfaces a hitch exactly when the user stopped
typing.

**Fix:** Hoist `save_draft`/`clear_draft` into `cx.background_spawn`; await or
fire-and-forget `.detach()`. (Refactor Step 3.x.)

### 2.4 `set_commit_main_diff` does open + diff + highlight all on UI thread — HIGH
**Evidence:** `src/ui/mod.rs:3523-3529` `Backend::open` +
`repo.commit_file_diff` inline, followed by `flat_map`/`filter`/`count` over
every hunk line twice (`:3546-3557`) plus the highlight parse.

**Fix:** One `cx.background_spawn` returning a fully-built `MainDiffView`
(content + highlights); apply on UI thread.

---

## 3. Repeated Git command risks

### 3.1 Repository re-opened per operation (no `RepoSession`) — HIGH
**Evidence:** `grep Backend::open src/` = **132 sites** (94 in `src/ui/`).
Trivial read paths re-open: `wip_diffstat` (`mod.rs:3907`), avatar coord
resolution (`avatar_fetch.rs:148`), file-history diff (`mod.rs:3242`), per-file
diff open (`mod.rs:3523`). `Backend::open` (`backend.rs:22-47`) re-reads
config, walks `.git` resolution, allocates pools.

**Cost:** ~1–5 ms per open, called on nearly every interaction. No
config/refs cache shared across opens.

**Fix:** One `RepoSession` per tab owns an `Rc<Backend>` (later `Arc` + worker
thread). Drop the 94 `src/ui/` open sites to a single field read. (Refactor
Step 2.1 — foundational; unblocks §1.4, §1.2 caching.)

### 3.2 Per-worktree re-open + full status on every snapshot — MEDIUM
**Evidence:** `src/git/snapshot.rs:322-344` loops `repo.worktrees()`, calls
`worktree_branch_name(path)` (`:362 Repository::open(path)`) and
`worktree_wip(path)` (`:374 Repository::open(path)` +
`working_tree_status(&repo)`). Each linked worktree = 2 opens + 1 full status.

**Cost:** N full working-tree scans per snapshot on multi-worktree repos.

**Fix:** Open each worktree once; skip WIP re-scan when last-known status is
unchanged (the watcher already debounces worktree events).

### 3.3 No ahead/behind cache; recomputed per snapshot per branch — MEDIUM
See §1.3. Cache and invalidate on fetch; compute lazily for visible sidebar
branches.

---

## 4. Caching opportunities

| Cache | Current | Proposed key | Invalidation |
|---|---|---|---|
| **Repository handle** | re-opened 132× | per-tab `RepoSession` | tab close |
| **Snapshot** | recomputed per reload | `(head_oid, status_mtime)` | watcher event classified Git/Index |
| **Per-file diff content** | ❌ none (file-list only) | `(commit_oid, path)` | repo reload |
| **Tree-sitter highlight** | recomputed per open | `(commit_oid, path, file_mtime)` | content change |
| **Graph layout** | recomputed per reload | `(head_oid, commit_count)` | commit-graph movement only (not status-only refresh) |
| **Graph per-row paths** | rebuilt per frame | per-row `Rc<[PathData]>` baked at snapshot | layout change |
| **Ahead/behind** | per branch per snapshot | `(branch_oid, upstream_oid)` | fetch |
| **Avatar color/initial** | recomputed per visible row per frame | on `CommitRow` at build | n/a |
| **Avatar bytes** | ✅ disk-cached (good — keep) | email hash | n/a |
| **Commit-row display strings** | ✅ pre-baked `SharedString` (good) | n/a | n/a |
| **`commit_row_index`** | ✅ `HashMap<CommitId, usize>` (good) | n/a | reload |
| **Theme/zoom** | atomic load ×1000/frame | one local per `render()` | setting change |

---

## 5. Incremental update opportunities

### 5.1 Graph layout: incremental on new commits
Today `layout(&snap.commits)` recomputes the whole graph. The lane-assignment
algorithm is amenable to incremental append: a new HEAD commit only affects
the first row's lanes; ancestors' lanes are unchanged. Cache the layout;
prepend the new row(s) on a commit-graph-movement event.

### 5.2 Status: scoped rescan
The watcher classifies events (`src/ui/watcher.rs:81-107`) and already
separates Index vs Git vs Worktree events. When a Worktree event fires for a
sub-path, run `git2::statuses` with that pathspec instead of a full scan.

### 5.3 Diff: reuse on click-back
The `diff_cache` already keys the file-list by row. Extend to key the
`FileDiffView` by `(row, file_index)`; populate on first open, return on
repeat. (Refactor Step 3.2.)

### 5.4 Sidebar: build once per snapshot, not per frame
**Evidence:** `src/ui/render.rs:585-594` — `self.sidebar_rows =
build_sidebar_rows(…)` rebuilds the entire sidebar flat list **every frame**.

**Fix:** Build in `apply_tab_view` (snapshot time); render reads it.

---

## 6. File watcher strategy

### 6.1 What's good (keep)
- `src/ui/watcher.rs:81-107` classifies events (skips `objects/`,
  `worktrees/`, `modules/`).
- `src/ui/tabs.rs:579-586` coalesces during a 500 ms `DEBOUNCE` window.
- Index/worktree events route to a cheap WIP refresh that skips the full
  reload.
- Architecture (separate Index vs Git handling) is well-thought-out.

### 6.2 What to fix
**Polling instead of blocking** — `src/ui/tabs.rs:556-557` `loop {
Timer::after(Duration::from_millis(100)).await; … match rx.try_recv() { Err(_) =>
continue } }`. The debounce comment in `watcher.rs:18` says it parks on a
500 ms timer, but the real loop wakes every 100 ms, immediately continues on
empty channel, then waits 500 ms more after the first real event.

**Fix:** Block on `rx.recv().await` (async channel) so the task wakes only on
a real event; debounce afterwards. Same for the single-instance listener
(`tabs.rs:646`, 200 ms poll) and auto-fetch ticker (`commands.rs:1474`).

**Detached tasks with no cancellation** — `tabs.rs:552-620`, `:643`,
`mod.rs:2570`, `commands.rs:1483` all `.detach()` loops relying solely on a
generation counter polled each iteration.

**Fix:** Store the `Task` on `KagiApp` (`watcher_task: Option<Task<()>>`),
replace on re-arm (cancels previous). Drop the polling loop for `rx.recv()`.

### 6.3 Proposed watcher pipeline
```
notify::Watcher → mpsc::rx.recv().await (wake on event only)
  → classify (Index / Git / Worktree / ignore)
  → debounce 500 ms (coalesce burst)
  → match classified:
       Index    → cheap WIP refresh (off-thread)
       Worktree → scoped status rescan (off-thread, pathspec)
       Git      → full snapshot (off-thread, §2.1)
  → apply on UI thread via cx.spawn + apply_tab_view
  → generation-gated (drop if tab switched)
```

---

## 7. Commit graph rendering strategy

This is the primary view and the hottest paint path.

### 7.1 Layout (compute once per commit-set change)
**Current:** full O(commits × lanes) recompute per reload (`commit_list.rs:189`).

**Proposed:**
- Store `GraphLayout` next to the snapshot on `TabViewState`.
- Cache key `(head_oid, commit_count)`.
- Invalidate only on commit-graph movement (new/removed commits), **not** on
  status-only refresh (which today triggers a full `reload()` → re-layout).
- Incremental path: on a single new HEAD commit, prepend its row; ancestor
  lanes unchanged.

### 7.2 Per-row path geometry (bake at snapshot time)
**Current:** `src/ui/graph_view.rs:228-316` — each row's `canvas(...)` paint
closure rebuilds a `PathBuilder` per edge (`:280,334,354`) and calls
`window.paint_path` per edge, per visible row, per frame. `lane_color` is
re-resolved per edge (`:274`). `stash_lanes: Vec<usize>` is cloned per row
(`render.rs:3327`).

**Proposed:**
- At snapshot time (in `build_commit_rows`), pre-bake per-row geometry into
  `Rc<[(Path, Hsla)]>` stored on `CommitRow`.
- The canvas paint closure strokes precomputed paths only.
- Lane color resolved once at build time.

**Impact:** Wide-branch repos (the `commit_list.rs:255` comment cites a
24-lane repo) are the dominant per-frame cost today; this removes it.

### 7.3 `uniform_list` item builder (O(1) per visible row)
**Current costs in the per-row builder (`render.rs:3161-3179`):**
- `let row = row.clone();` (`:3179`) clones the entire `CommitRow`
  (`author_email: String`, `badges: Vec<RefBadge>`, `edges: Vec<GraphEdge>`,
  `parents: Vec<CommitId>`) per visible row per frame — even though handlers
  only use `ix`.
- `row.edges.clone()` (`:3321`), `stash_lanes.to_vec()` (`:3327`) re-clone
  inside the row builder.
- `avatar_color` + `avatar_initial` (`:3218-3219`) recompute from the raw
  email/name string per visible row per frame.
- `theme()` called 4× per row (`:3186,3188,3190,3253`).
- `cx.listener(...)` allocated twice per row (`:3199` click, `:3210` context);
  a per-row scroll-wheel listener (`:3312`) is identical for every row but
  allocated N times.

**Proposed:**
- Drop `let row = row.clone();` — handlers use `ix`; borrow `&rows[ix]` in the
  builder body.
- Change `graph_canvas` to borrow (`edges: &[GraphEdge]`, `stash_lanes:
  &[usize]`).
- Pre-compute `avatar_color`/`avatar_initial` onto `CommitRow` at build time
  (the comment at `commit_list.rs:140-142` claims this but avatar color is
  NOT pre-computed).
- Hoist `let theme = theme::theme();` to one local per `render_rows` call.
- Hoist identical listeners (scroll-wheel) to the column `div()` level.

---

## 8. Diff rendering strategy

### 8.1 Two-phase render (text first, highlights async)
**Current:** `highlight_diff_rows` runs synchronously in the open-diff path
(`mod.rs:3291`).

**Proposed:**
1. On open: `cx.background_spawn` the git2 diff → build `MainDiffView` rows
   without highlights → apply on UI thread → paint text immediately.
2. `cx.background_spawn` `highlight_diff_rows` → when done, swap highlighted
   rows into the `MainDiffView` and `cx.notify()`.

### 8.2 Content cache by `(commit_oid, path)`
Today only the file-list is cached (`diff_cache` keyed by row). Extend with
`file_diff_cache: HashMap<(usize, usize), Arc<FileDiffView>>` (row +
file-index). Invalidate together with `diff_cache` on repo reload.

### 8.3 Arc-wrap the view
**Current:** `src/ui/render.rs:567 main_diff = self.main_diff.clone()` clones
a `Vec<DiffRow>` of highlighted rows every frame. Same for `compare_view`
(`:568`), `conflict` (`:627`).

**Proposed:** Store as `Arc<MainDiffView>`; render clones only the `Arc`.

### 8.4 Windowed diff rendering
For very large diffs (>10k lines), render only the visible scroll range with
`uniform_list` (already used for the commit list). Today the full diff is
built into `MainDiffView.rows` and rendered in one `div` tree.

---

## 9. Async / task-lifecycle strategy

### 9.1 Marshal-back pattern (mostly correct — one gap)
The `cx.background_spawn` + `cx.spawn` marshal-back pattern in
`src/ui/operations/*` is idiomatic and correct. **Gap:** the marshal closures
discard the generation check:

```rust
let _ = this.update(acx, |app, cx| { … cx.notify(); });   // generation discarded
```

(28+ sites: `branch.rs:860,1009,1138,1378`, `checkout.rs:409`, `commit.rs:878`,
`cherry_revert.rs:105,244`, `pull_push.rs:268,469`, `stash.rs:128,776`,
`discard.rs:171`, `worktree.rs:204`, `history.rs:690`, `mod.rs:1837,2071,…`.)

**Risk:** A long-running push that completes after a tab switch will
`reload()` the **wrong** repo, clobbering the current view. Only
`file_history` and the tab switcher actually check a generation.

**Fix:** Capture `let gen = self.switch_generation;` when spawning; inside
the marshal closure, `if app.switch_generation != gen { return; }` before
side effects.

### 9.2 Auto-fetch: one ticker per remote-URL, not per tab
**Current:** `src/ui/commands.rs:43 AUTO_FETCH_INTERVAL_SECS = 180`;
`:1474-1490 ensure_auto_fetch_ticker` spawns a `cx.spawn` loop per tab,
called unconditionally per render at `src/ui/render.rs:438`. N tabs = N
fetches every 3 min, possibly to the same remote.

**Fix:** One global ticker per remote-URL, armed on app init (not from
render). Dedupe across tabs.

### 9.3 Unified task manager
Nine ad-hoc concurrency fields (`busy_op`, `fetch_in_flight`,
`toast_ticker_alive`, `auto_fetch_ticker_alive`, `refresh_spin_started`,
`modal_replan_gen`, `draft_save_gen`, `switch_generation`,
`watcher_generation`) each implement "spawn with generation; compare on
completion." Consolidate into a `BackgroundTasks` helper: `spawn(name, F)`,
`is_busy(name)`, `cancel(name)`.

---

## 10. GPUI render-path micro-optimizations

### 10.1 `theme()` / `scaled_px()` called hundreds of times per frame
**Evidence:** `src/ui/theme.rs:146-153` (`theme()` atomic load), `:215,241,254`
(`scaled`/`zoom`). Render calls `theme()` on essentially every `div().bg(rgb(
theme().X))`. Comment at `theme.rs:187-188`: "`text_sm`/`text_xs` 260+ times."
`render.rs:402` reapplies `set_rem_size` every frame.

**Fix:** Bind `let theme = theme::theme(); let z = theme::zoom();` once at the
top of `render()`; pass into helpers. Gate `set_rem_size` behind
`if window.rem_size() != px(rem_size_px())`.

### 10.2 Render-time state mutation
**Evidence:** `render.rs:402` (`set_rem_size`), `:409-419`
(`self.bottom_panel_height = …`), `:446` (`self.history_seed_attempted = true;
seed_history_from_reflog`), `:468-480` (`pending_smart_msg.take()` →
`set_template_inputs` → notify), `:485-497` (`graph_scroll_x = max`), `:585-594`
(`sidebar_rows = build_sidebar_rows` every frame).

**Fix:** Move lazy-init to `fn reconcile(&mut self, window, cx)` invoked from
`cx.observe_self` or the event handlers that change inputs. Render should be
pure presentation.

### 10.3 `eprintln!` + unconditional atomic on hot paths
**Evidence:** `render.rs:457-463` — per-50-frame counter does
`AtomicU64::fetch_add` on every render even when the env var is unset (the
`if` is after the fetch). `render.rs:413`, `mod.rs:179,2061-2065,2077-2080`,
`operations/commit.rs:289-294,3558`.

**Fix:** Gate the `fetch_add` behind the env-var check. Replace `eprintln!` on
hot paths with the `klog!` macro.

### 10.4 `Rc<Cell<(f32,f32)>>` geometry backchannels
Six fields (`mod.rs:943,948,1082,1085` + locals) smuggle paint-time bounds into
drag handlers via interior mutability. Not a perf issue per se, but a
correctness/fragility issue (stale coords → divider jumps on first drag).

**Fix:** Store last-known `Bounds<Pixels>` in a normal `Option<Bounds<Pixels>>`
field, updated from the canvas **prepaint** closure (runs during layout,
before paint). Or query live bounds from the drag handler.

---

## 11. Prioritized fix order (value / effort)

| # | Fix | Severity | Effort | Depends on |
|---|---|---|---|---|
| 1 | `reload_external` off-thread (§2.1) | Critical | Small | — |
| 2 | Per-file diff content cache (§1.4, §8.2) | High | Small | — |
| 3 | Tree-sitter highlight off-thread (§1.5, §8.1) | High | Small | — |
| 4 | Arc-wrap `MainDiffView`/`compare_view`/`conflict` (§8.3) | High | Small | — |
| 5 | Drop `row.clone()` in `render_rows`; `graph_canvas` borrows (§7.3) | High | Small | — |
| 6 | Cache avatar color on `CommitRow` (§7.3) | Medium | Small | — |
| 7 | Hoist `theme()`/`zoom()` to one local per render (§10.1) | Medium | Small | — |
| 8 | Graph layout cache by `(head_oid, commit_count)` (§1.2, §7.1) | High | Medium | RepoSession |
| 9 | Pre-bake per-row graph paths (§7.2) | Medium | Medium | #8 |
| 10 | One `RepoSession` owns the Backend (§3.1) | High | Medium | — (foundational) |
| 11 | Lazy ahead/behind (§1.3) | Medium | Medium | — |
| 12 | Single auto-fetch ticker per remote-URL (§9.2) | Medium | Small | — |
| 13 | Watcher: block on recv; store Task for cancel (§6.2) | Low | Small | — |
| 14 | Generation check in marshal closures (§9.1) | Medium | Small | — |
| 15 | Decompose `KagiApp` into child `Entity<T>` panels (root cause) | Critical | Large | RepoSession |
| 16 | Worker thread per RepoSession (§3.1 long-term) | High | Large | RepoSession |

**Tactical bundle (one week, no architectural deps):** #1, #2, #3, #4, #5, #6,
#7, #12, #13, #14 — all Small effort, high combined impact, independent.

**Strategic (unblocks the rest):** #10 (RepoSession) → #8, #9, #15, #16.

---

## 12. What's already well-engineered (preserve during refactors)

- The `cx.background_spawn` + `cx.spawn` marshal-back pattern in
  `src/ui/operations/*` (the gap in §9.1 is a missing check, not a wrong
  pattern).
- Avatar disk-cache + off-thread HTTP (`src/ui/avatar_fetch.rs`):
  `:443 resolve_avatars` runs off-thread, `:272-285` checks disk cache first,
  `:207` persists, 10 s timeout.
- Watcher event classification/debounce (`src/ui/watcher.rs:81-107`) — the
  architecture is sound; only the polling loop and missing cancellation need
  fixing.
- `commit_row_index: HashMap<CommitId, usize>` (`mod.rs:1199`) — the right
  lookup structure; avoids O(n) per-commit searches.
- Pre-baked `SharedString` display fields on `CommitRow` (`commit_list.rs`)
  — extend this pattern to avatar color (§7.3).
- `menu_overlay.rs` and `button_style.rs` — real de-duplication (15+ call
  sites), not ceremony.
- The release profile tuning (`Cargo.toml`: `lto = "thin"`,
  `codegen-units = 1`, `strip = true`) and the dev-profile opt-level bumps
  for `gpui`/`taffy`/`rustybuzz`/`ttf-parser` — correct calls.
- Safe-mode checkout (`cb.safe()`) used consistently — never `cb.force()`
  (except the intended `checkout_index` in discard).

---

*Cross-reference: `/docs/codebase-review.md` §3 (full findings list),
`/docs/refactor-plan.md` Phase 3 (executable perf steps).*
