# 05 — Conflict Resolution (Conflict Mode / 3-pane editor / dashboard / flow)

> NOTE (2026-06-22): `src/git` was extracted to `crates/kagi-git` in ADR-0115; paths below describe the pre-extraction layout.

Re-architecture research (sub-agent #5). Scope: Conflict Mode state machine, the
3-pane conflict editor, file/chunk/line-level accept, live Result preview/edit,
the conflict dashboard, the Save→stage / Continue / Abort / Skip flow, and
merge-into-conflict.

Layering target: **domain** (pure: hunk model, conflict parse, A/B/line
resolution → assembled text, session state machine) → **git-backend**
(continue/abort/skip as pipeline operations; in-memory dry-run) → **app**
(AppState conflict session + autosave) → **ui** (dashboard + 3-pane editor
view-models + views). Invariants: UI never calls git2 directly; abort always
restores the exact pre-op state; the resolution is autosaved (never lose partial
work — the jj principle, ADR-0057).

Builds on the existing conflict-ux research (`docs/research/conflict-ux-models.md`,
`conflict-ux-gui-clients.md`, `conflict-ux-editors.md`) — synthesizes, does not
redo it.

---

## 1. Kagi 現状

The conflict feature is already cleanly **three-layered in the backend** but the
**UI half is heavily tangled into `KagiApp`** (~15 fields, ~27 methods, geometry
via `Rc<Cell>`, and a narrow git2 leak). The pure domain logic and the safe
pipeline ops are the strongest part of the existing code; the binding to the app
shell is the weakest.

### 1.1 domain — pure model (already well-isolated)

Lives in **`src/git/resolution.rs`** (2139 LOC, almost entirely pure) and the
pure half of **`src/git/conflicts.rs`** (1644 LOC):

- **Hunk model** (`resolution.rs`) — the heart, already a testable pure type:
  - `HunkModel { regions: Vec<Region> }`, `Region::{Passthrough(Vec<String>),
    Hunk(ConflictHunk)}`.
  - `ConflictHunk { current, incoming, base: Vec<String>, choice: HunkChoice,
    line_select: Option<LineSelection> }`.
  - `HunkChoice::{AcceptCurrent, AcceptIncoming, BothCurrentFirst,
    BothIncomingFirst, Manual(String), Unresolved}`.
  - `HunkModel::from_marker_text(&str)` — parses zdiff3 / standard markers
    (`<<<<<<<` / `|||||||` / `=======` / `>>>>>>>`) into ordered regions, split
    on `\n` over `&str`, recovers malformed/truncated hunks. `assemble() ->
    Vec<ResolvedLine>` and `assembled_text()` re-emit the file with per-line
    provenance; an `Unresolved` hunk re-emits markers so the residue gate trips.
  - **Line-level (ADR-0071)** — `LineSelection { current_taken: Vec<bool>,
    incoming_taken: Vec<bool>, order: LineOrder }`, `TriState::{All,Partial,None}`
    with `from_bools`/`from_children` for file/chunk/line tri-state propagation,
    `SelectionSide::{Current,Incoming}`. `ensure_line_selection` seeds line state
    from the existing hunk choice so chunk→line is lossless and backward
    compatible (choice-driven when `line_select == None`).
- **Resolution buffer** (`resolution.rs`) — `ResolutionBuffer { repo_path,
  files: BTreeMap<PathBuf, FileResolution>, hunks: BTreeMap<PathBuf, HunkModel> }`.
  In-memory only (WT/index untouched). `from_repo` materializes side texts +
  `materialized_markers` (zdiff3 via `merge_file_from_index`, standard fallback).
  Per-file `apply_choice`/`set_manual_text` + per-file undo/redo stack
  (`FileResolution.undo/redo`), and the hunk/line API (`apply_hunk_choice`,
  `set_file_side_selection`, `set_hunk_side_selection`, `set_hunk_line_selection`,
  `set_hunk_line_order`, `reset_hunk`) which re-assembles and checkpoints into the
  file Result. Queries: `has_resolution`, `resolved_text`, `provenance`,
  `files_with_marker_residue` (shares `checklist::text_has_conflict_marker`).
  **Autosave** is hand-written serde-free JSON to `~/.kagi/conflicts/<sha1(repo)>/
  buffer.json` (or `$KAGI_LOG_DIR`), load/clear lenient like `drafts.rs`.
- **Session model** (`conflicts.rs`) — `ConflictSession { op: ConflictOp,
  files: Vec<ConflictFile> }`, `ConflictOp::{Merge,Rebase{step,total},CherryPick,
  Revert}`, `ConflictKind::{Content,RenameDelete,ModifyDelete,Binary}`,
  `ConflictStatus::{Unresolved,Resolved,NeedsReview}`. `detect_conflict_session`
  reads `Repository::state()` + `Index::conflicts()` + `.git/` state files
  (rebase `msgnum`/`end`, `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`,
  `stopped-sha`). **Terminology (ADR-0058)** — `side_labels` returns role+name
  pairs and translates the rebase ours/theirs swap to "New base" / "Your commit
  being replayed"; the words ours/theirs never appear in any user string.

> Note: `detect_conflict_session` and `ResolutionBuffer::from_repo` need a live
> `&git2::Repository` — they read the index. So "pure domain" here means *the
> model and assembler are pure*; **detection/materialization are git-backend reads**,
> not domain. The re-arch should keep that split: `HunkModel`/assemble/parse +
> session-FSM transitions are domain; detection + zdiff3 materialization are
> git-backend adapters that *produce* domain values.

### 1.2 git-backend — continue/abort/skip as plan-pipeline ops (good)

In `conflicts.rs`, all flow ops are `plan_* → execute_*` on the existing
`OperationPlan` pipeline, repo read-only until execute:

- `continue_blockers(repo, session, buffer) -> Vec<ContinueBlocker>` — single
  source of truth for the ADR-0067 checklist (`UnresolvedFiles`, `MarkerResidue`,
  `IndexUnmerged`, `BinaryUnresolved`, `DeletionUndecided`, `EmptyMergeMessage`,
  `ChecklistBlocker`). Shared by the plan modal and the UI gate.
- `plan_conflict_continue` / `execute_conflict_continue` (materialize buffer → WT
  → `index.add_path` (stage 0); merge creates the 2-parent commit + `cleanup_state`,
  sequencers return `Staged`). `plan_conflict_continue_route` → `ContinueRoute::
  {MergeCommitPanel{message}, SequencerPlan(OperationPlan)}` (ADR-0068 — merge
  does NOT commit on Continue; it routes to the commit panel).
- `execute_conflict_save` — per-file Save→stage with a hard marker-residue block
  (ADR-0066/0068). `execute_merge_commit` — the commit-panel sink, refuses on
  remaining unmerged entries.
- `plan_conflict_abort` / `execute_conflict_abort` — **safe restore, no
  `reset --hard`/`clean`/force**: read ORIG_HEAD tree into the index to drop
  conflict stages, then per-path rewrite each session file's pre-op blob (or
  remove if absent), move the branch ref back to ORIG_HEAD, `cleanup_state`. The
  buffer is `autosave()`-flushed *before* touching the repo (ADR-0057 recovery).
- `plan_conflict_skip` / `execute_conflict_skip` — sequencer-only (merge errors).

### 1.3 ui — `conflict_view.rs` + `conflict_editor.rs` + the KagiApp tangle

- **`conflict_view.rs`** (1141 LOC) — holds `ConflictMode { session, buffer,
  current_branch, selected_file, editing_file, abort_armed }` (the UI-side mode
  snapshot, **cloned** into render), `EditorInputs { path, result:
  Entity<InputState> }`, and `EditorChrome` (the per-render bundle threading the
  editor InputState + split ratios + Result mode + the two `Rc<Cell>` geometry
  cells + scroll handle + `selected_hunk` into the view). `ConflictMode` carries a
  **UI-side duplicate of the continue gate** (`can_continue`/`continue_blocker`
  returning a localized `Msg`) — a buffer-only subset of `continue_blockers`.
  Renders banner + dashboard (header, role badges, counts+nav, actions
  Abort/Continue/Skip, conflicted/resolved sections, escape hatch).
- **`conflict_editor.rs`** (1017 LOC) — the 3-pane editor (ADR-0064/0069/0071).
  A|B are **row-lists with tri-state checkboxes** (`uniform_list`, shared
  `ScrollHandle` for ADR-0070 sync — NOT InputState, per the ADR-0069→0071
  revision: InputState can't host a per-line checkbox gutter). The Result pane is
  Preview (custom monospace rows) / Edit (`InputState`). All button handlers call
  back into `KagiApp` `conflict_editor_*` methods. No git2 here.
- **`KagiApp` tangle (`src/ui/mod.rs`, 16775 LOC) — the core problem:**
  - **~15 conflict-named fields** (delimited by a banner comment): `conflict:
    Option<ConflictMode>`, `conflict_detected_for: Option<PathBuf>` (run-once
    guard), `conflict_editing: Option<PathBuf>`, `conflict_editing_before_text:
    HashMap<PathBuf,String>` (before→after hash logging),
    `conflict_editor_inputs: Option<ConflictEditorInputs>`,
    `conflict_result_editing: bool`, `conflict_reset_all_armed: bool`,
    `conflict_ab_split: f32`, `conflict_result_split: f32`,
    **`conflict_geom: Rc<Cell<(f32,f32)>>`**, **`conflict_ab_geom:
    Rc<Cell<(f32,f32)>>`**, `conflict_merge_commit_pending: bool`,
    `conflict_selected_hunk: usize`, `conflict_ab_scroll_handle:
    UniformListScrollHandle`, `conflict_continue_modal:
    Option<ConflictContinuePlanModal>`. Init is **duplicated across two
    constructors**.
  - **~27 methods** — detection/lifecycle (`detect_conflict_mode`,
    `conflict_open_editor`, `conflict_select_file`, `conflict_nav_unresolved`),
    resolution (`conflict_apply_choice`, `conflict_editor_set_file_side` /
    `_set_hunk_side` / `_set_hunk_line` / `_set_hunk_order` / `_select_hunk`),
    flow (`conflict_continue`/`confirm_`/`cancel_`, `conflict_abort`/`_request`,
    `conflict_skip`, `conflict_editor_save`, `finish_merge_commit`), chrome
    (`_toggle_result_mode`, `_nav_hunk`, `_reset_all`/`_request`, `_open_external`),
    escape hatch (`_open_external_tool`, `_open_terminal`, `_copy_path`,
    `_copy_git_command`).
  - **git2 leak into UI (invariant violation, but narrow):** ~8 conflict methods
    do `git2::Repository::open(...)` inline, but **only to forward the handle** to
    a `kagi::git::*` function or a `ResolutionBuffer` method — no direct git2
    mutation/query logic lives in UI. The leak is *structural* (UI owns repo-open
    + error→toast + `Repository` lifetime, and the same ~10-line preamble is
    copy-pasted per handler), not logical. Prime extraction target.
  - **Geometry via `Rc<Cell>`:** the two cells are written from a `canvas` measure
    closure inside `conflict_editor.rs` (`ab_geom.set(...)`, `geom.set(...)`) and
    read in the single root divider-drag listener (`DividerKind::{ConflictAB,
    ConflictResult}` → `conflict_split_ratio_from_cursor`). A render→drag
    back-channel that bypasses normal state flow (W7 inspector-split pattern,
    ADR-0070).
  - **Autosave is eager/inline, no debounce** — `buffer.autosave()` is called at
    5 sites right after each mutation; the Result Edit path can fire **per render
    frame** (sync runs each render when text differs). **This contradicts
    ADR-0057's "250ms debounce"** — see §5.
  - **Render coupling:** `EditorChrome` + the backing `InputState`s are rebuilt
    every frame (`sync_conflict_editor_inputs`, gated by a `content_sig`) because
    the editors live on `self` while `ConflictMode` is cloned for render. The
    `conflict_merge_commit_pending` flag swaps the body back to the *normal*
    layout so the commit panel shows for the merge-commit step.
  - **Entry:** `render` calls `detect_conflict_mode()` each frame, made run-once
    by `conflict_detected_for`; `reload()` resets the guard (FS watcher / op
    completion re-detects). Merge-into-conflict (`execute_merge_into_conflict`)
    leaves the repo conflicted and the next reload re-detects.

---

## 2. 参考プロジェクトの実装方針

(Synthesized from the three existing conflict-ux research docs + Zed/GitKraken.)

- **jj (思想ソース)** — first-class conflict in the commit object (`Merge<T>`
  tree列), `simplify()` makes rebase always succeed, partial resolution never
  lost via op-log snapshots, ours/theirs never used (always "side X"). Borrow the
  *principles* (never lose partial work; show 3-way base context; translate
  ours/theirs to context names) — **not** the object model (breaks standard-Git
  interop; conflicts with the git2-single-backend invariant).
- **GitButler** — synthetic-root-tree conflicted commits; FSL license →
  concept-only, no code reuse.
- **GitKraken** — internal 3-way + live Output preview + scroll sync; "current /
  target" human-language labels (not ours/theirs); file/chunk/line + Take-All +
  manual edit (the 4-granularity model Kagi adopts via ADR-0071); known weak at
  binary/rename-delete and slow on large change-sets.
- **Fork** — best ours/theirs avoidance: shows real branch names; 2-way↔3-way
  toggle; multi-conflict batch resolve. Weakness Kagi already fixes: resolved
  state not visible in the list.
- **SourceTree / GitHub Desktop** — mostly external-tool delegation; SourceTree's
  historic *label-inversion bug* (Resolve-Using-Mine applied theirs) is the
  cautionary tale that justifies Kagi's role-label translation. Both teach: keep
  a Continue/Abort banner whose exit is always visible; same resolve UI for
  merge/rebase/cherry-pick.
- **Zed conflict/merge editor (gpui native, GPL — concept-only, ADR-0069)** —
  conflict markers parsed into the multibuffer as decorated regions with
  Accept-Current / Accept-Incoming / Accept-Both inline actions; a single editor
  buffer with per-conflict overlays rather than a 3-pane split. Confirms that a
  *region list over the file* (Kagi's `HunkModel`) is the right domain shape; we
  diverge on presentation (Kagi keeps the explicit A|B|Result 3-pane + checkbox
  gutter per ADR-0064/0071, which is more discoverable for a safety-first GUI than
  inline-only actions).

---

## 3. 採用すべき設計

### 3.1 domain — pure conflict crate (lift `resolution.rs` mostly as-is)

The hunk model + assembler + line-selection + tri-state are *already* a clean,
fully-testable pure module — **promote them verbatim into the domain layer**.
Add the **session FSM as a first-class testable type** that today only exists
implicitly across `conflicts.rs` + `ConflictMode`:

```
ConflictSessionState (domain, pure)
  ├ op: ConflictOp                 // Merge / Rebase{step,total} / CherryPick / Revert
  ├ files: Vec<ConflictFile>       // kind + status (derived from buffer)
  ├ buffer: ResolutionBuffer       // in-memory drafts (lifted as-is)
  └ derived (pure, recomputed):
      can_continue / continue_blockers (buffer-only subset)
      can_abort = true (always)     // safety valve
      can_skip = op.is_sequencer()
```

- The transitions are pure functions: `apply_choice`, `apply_hunk_choice`,
  line/file/chunk selection, `set_manual_text`, undo/redo, and `recompute_status`
  (status derived from `has_resolution` + residue). This is exactly what
  ADR-0062's `ConflictResolutionSession` asked for ("derived values the UI only
  reads"), and it kills the **duplicate gate** in `ConflictMode::can_continue`
  (UI computes the gate itself today) — UI reads `session.can_continue` instead.
- Keep `side_labels` (terminology translation) in domain — pure, no repo.
- Marker parse/assemble round-trip is the single most test-worthy unit; keep
  `text_has_conflict_marker` shared with the commit checklist.

### 3.2 git-backend — detection adapters + flow ops (keep, dedupe the preamble)

- `detect_conflict_session` and `ResolutionBuffer::from_repo` /
  `materialized_markers` are **git-backend reads** that *produce* domain values
  (they need `&Repository`). Keep them backend-side; the domain never sees git2.
- `continue_blockers` / `plan_*` / `execute_*` for save/continue/abort/skip and
  `execute_merge_commit` stay as plan-pipeline ops (already correct). Add an
  **in-memory dry-run** for continue (write the assembled trees into an in-memory
  index via `merge_trees`-style preview) so the dashboard can show the predicted
  result before the user commits — consistent with the rest of the re-arch's
  "predict before mutate" invariant.
- Provide a **backend façade** (`ConflictBackend`/repo-handle service) so the app
  layer calls `backend.continue(session, buffer)` etc. without ever holding a
  `git2::Repository`. This removes the ~8 inline `Repository::open` sites and the
  copy-pasted open+error-toast preamble from the UI — closing the git2-leak
  invariant violation.

### 3.3 app — AppState conflict sub-state + debounced autosave

- Model **Conflict Mode as an explicit app sub-state**, not 15 loose fields:
  `RepoMode::{Normal, Conflict(ConflictAppState)}` (ADR-0056's intent). The app
  state owns: the domain `ConflictSessionState`, `editing_file`, the
  `merge_commit_pending` flow flag, and the continue-plan modal. UI chrome that
  is purely presentational (split ratios, Result Edit mode, armed flags,
  selected_hunk, scroll handle, geometry cells, InputState entities) moves to a
  **view-owned editor view-model** (§3.4), not AppState.
- **Autosave: restore the ADR-0057 250ms debounce.** Today it is eager/inline and
  the Result-edit path can write per frame. Use the same debounce-generation
  pattern the commit-draft layer already uses (`draft_save_gen`) so a burst of
  line toggles or typing coalesces into one write.
- Detection stays a run-once-per-reload app task (the `conflict_detected_for`
  guard generalizes to the app-state mode transition); merge/rebase/cherry-pick
  completion and the FS watcher trigger re-detection uniformly.

### 3.4 ui — self-contained 3-pane editor component with its own view-model

- The dashboard and the 3-pane editor become **self-contained view components**
  each with their own view-model derived from `ConflictAppState` — they read the
  domain session and emit *intents* (e.g. `AcceptHunkSide{path,hunk,side,taken}`,
  `Continue`, `Abort`, `Save{path}`) that the app handles. No view method opens a
  repo or calls a `plan_*`.
- The editor view-model owns the presentation-only state currently scattered on
  KagiApp: split ratios, Result Preview/Edit, armed flags, `selected_hunk`, the
  shared scroll handle, the InputState entities, and the geometry cells. The
  `EditorChrome` bundle becomes that view-model. The `Rc<Cell>` measure→drag
  back-channel stays (it's the established gpui split-resize idiom, ADR-0070) but
  is **encapsulated in the editor component** rather than living as two fields on
  the global app.
- Same resolve UI for merge/rebase/cherry-pick/revert (only labels + Skip
  visibility differ) — already true; preserve it.

---

## 4. 採用しない設計

- **First-class conflict in the commit object** (jj/GitButler `Merge<T>` /
  synthetic root tree) — breaks standard-Git interop and the git2-single-backend
  invariant (ADR-0005/0023/0031). Git-CLI model only.
- **Fearless / always-succeeding rebase** — depends on first-class conflict.
  Keep CLI-style pause + Continue/Abort/Skip.
- **AI auto-resolution in initial scope** — GitKraken-style auto-merge; high
  verification cost vs. the safety-first stance. If ever added, propose→human
  approves, non-destructive only.
- **InputState-based A/B panes** (original ADR-0069) — already superseded by
  ADR-0071's row-list-with-checkbox-gutter; do not regress to InputState A/B.
- **External-tool-only resolution** (SourceTree/GitHub Desktop) — keep the
  built-in 3-pane; external tool stays an escape hatch, not the primary path.
- **`reset --hard` / `clean` / force for abort** — the targeted per-path ORIG_HEAD
  restore must stay (data-safety invariant).

---

## 5. リスク (data-loss surfaces)

1. **Autosave debounce regression (active discrepancy).** Implementation autosaves
   eagerly/inline (5 sites; Result-edit can write per frame), but ADR-0057
   mandates 250ms debounce. Eager is *safer for data* but a perf/IO smell;
   debouncing re-introduces a window where a crash mid-burst loses the last <250ms
   of edits. Decide explicitly and document; flush on focus-loss / mode-exit /
   abort.
2. **Abort restore correctness.** `execute_conflict_abort` is the highest-stakes
   path: it rewrites WT files per-path from the ORIG_HEAD tree and moves the
   branch ref. Risks — a session file modified *outside* the operation, files
   added by the operation (removed on abort), and the IndexUnmerged blocker not
   catching paths outside `session.files`. Must keep buffer-flush-before-touch and
   exhaustive tests for merge/rebase/cherry-pick/revert × content/rename-delete/
   modify-delete/binary.
3. **Marker residue reaching a commit.** Defended at Save (hard block), continue
   gate, and a defensive re-check in `execute_conflict_continue`. Keep all three;
   the `Unresolved` hunk re-emitting markers is the mechanism that makes an
   unfinished Result trip the gate — do not "optimize" it away.
4. **Continue path-loss / stage mismatch.** `execute_conflict_continue` stages only
   `session.files`; an unmerged path outside the session (e.g. detected after a
   re-scan) must block (IndexUnmerged) rather than be silently committed.
5. **merge-into-conflict re-entry.** After `execute_merge_into_conflict`, the mode
   must re-detect deterministically; a missed re-detect leaves the user in a
   half-committed state with a stale `ConflictMode`.
6. **Per-frame Result-edit sync clobbering in-progress edits.** The `content_sig`
   gate prevents rebuilding the InputState over live typing — the re-arch must
   preserve that guard when the editor becomes a component.
7. **Buffer/index divergence.** The buffer is in-memory; Save stages to index.
   External CLI edits between detect and continue can desync — `continue_blockers`
   re-reads the live index, but the buffer's materialized sides are snapshot at
   detect; a re-scan must rebuild from the live index, preferring the autosaved
   Result drafts (current `load().or_else(from_repo)` order is correct).

---

## 6. 未解決事項

1. **Autosave policy** — eager-inline (current, safest) vs. ADR-0057 250ms
   debounce. Recommend debounce + flush-on-exit; needs a decision + ADR amendment.
2. **In-memory continue dry-run** — should the dashboard show a predicted merged
   result (via `merge_trees`) before commit? Adds backend work; aligns with the
   re-arch "predict before mutate" invariant. Scope TBD.
3. **Where detection + zdiff3 materialization live** — confirmed as git-backend
   adapters producing domain values, but the buffer currently *owns*
   `materialized_markers(repo, ...)` (takes `&Repository`). Re-arch should move
   that read out of `ResolutionBuffer` into a backend function returning marker
   text, so the buffer is 100% pure/serializable.
4. **Geometry `Rc<Cell>` encapsulation** — keep the measure→drag idiom but where
   does it live once the editor is a component? (Likely on the editor view-model,
   but the root drag listener currently dispatches by `DividerKind` globally.)
5. **Session id / log correlation** — ADR-0062 specifies a `ConflictSessionId`
   (`sha1(repo+op+started_at)`) to correlate oplog + resolution-log entries; not
   yet implemented. Decide if the re-arch introduces it now.
6. **Skip executor depth** — `execute_conflict_skip` advances the sequencer; the
   multi-commit rebase continuation driver ("commit + advance to next pick" noted
   as a later lane in `execute_conflict_continue`) needs to be fully owned by the
   backend sequencer, not the UI.
7. **rename-delete / modify-delete / binary card UI** — ADR-0059 wants dedicated
   card UI (the differentiator all GUIs are weak at); current code classifies the
   kinds but the editor falls back to a generic guidance pane. Re-arch component
   boundaries should leave room for per-kind cards.
8. **Status recompute ownership** — today the UI recomputes per-file
   `ConflictStatus` from the buffer at detect + after each mutation (duplicated
   logic in `detect_conflict_mode` and the `conflict_view` test helper). Move this
   to a single domain `recompute_status` on the session FSM.
