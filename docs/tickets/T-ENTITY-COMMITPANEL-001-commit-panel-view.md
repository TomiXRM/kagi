# T-ENTITY-COMMITPANEL-001 — `CommitPanelState` → `Entity<CommitPanelView>`

- ADR: 0118 (KagiApp decomposition, Phase 5.2) — Mechanism B (Entity promotion), the LAST remainder
  (highest coupling). Uses the fat-entity template proven by `ConflictView` (T-ENTITY-CONFLICT-001,
  landed) + `FileHistoryView` (ADR-0117).
- Risk: **HIGH** (commit creation is a write op, ADR-0104 pipeline; nested `Entity<InputState>`
  inputs; smart-commit CLI/LLM; per-branch draft autosave; the conflict→merge-commit cross-flow).
- Owner: SubAgent per stage → PM verify + adversarial codex cross-review + PM real-UI verification.
- Status: design — codex adversarial cross-plan review done (DESIGN-NEEDS-CHANGES → corrections
  folded in below, see "Cross-review corrections").

## Current shape (from the structural map)
- `KagiApp`: `commit_panel: Option<CommitPanelState>` (`mod.rs:927`, Some=open), `commit_panel_open:
  bool` (`:925`, WIP-row-selected gate set by graph `select`), `commit_input: Option<Entity<
  InputState>>` (`:931`), `commit_template_mode: bool` (`:936`), `commit_template_inputs:
  Option<[Entity<InputState>;6]>` (`:941`), `smart_commit: SmartCommitState` (`:944`),
  `smart_commit_detected_for: Option<PathBuf>` (`:946`), `pending_smart_msg: Option<String>`
  (`:949`), `last_draft_value: String` + `draft_save_gen: u64` (`:1074/1076`), `file_menu` (`:1094`,
  shared overlay).
- `CommitPanelState` (`commit_panel.rs:55`): 11 data fields (staged/unstaged + stats + trees +
  indices, conflicted_paths, selected_file, commit_msg fallback, plan_modal, tree_view, preview).
- Render: `render_commit_panel(&self, panel, width, preview, cx: &mut Context<KagiApp>)`
  (`commit_panel_render.rs:487`), called from `render_body` (`render_body.rs:444`); per-row free fns
  `render_{unstaged,staged}_{flat,tree}_row(this: &KagiApp, …)`. **22 `cx.listener(|this: &mut
  KagiApp|)`** in commit_panel_render.rs.
- Backend (write): `open_commit_plan_modal`→`plan_commit`; `start_commit` (async `cx.background_spawn`
  + reload + record_op); `finish_merge_commit` (sync `repo.run(MergeCommit)` + reload); `do_stage_file
  /do_unstage_file/do_stage_all/do_unstage_all/discard_all`; smart-commit gen (async, **no generation
  guard** — map flag); draft autosave (`draft_save_gen` guard).
- Cross-flow: `conflict_continue` (parent) → `open_commit_panel` + set `commit_input` value +
  `conflict_merge_pending=true`; `start_commit` checks `conflict_merge_pending` → `finish_merge_commit`.
- Lifecycle: `reset_per_repo_ui`/`show_welcome` ALREADY drop `commit_panel`/`commit_input`/template
  (`tabs.rs:346-352`) — good, unlike conflict. `KAGI_COMMIT_PANEL=1` headless scenario exists.

## Proposed design (fat entity + deferred marshalling — same as ConflictView)
1. `KagiApp.commit_panel: Option<CommitPanelState>` → `Option<Entity<commit_panel::CommitPanelView>>`.
   `CommitPanelView` owns: the `CommitPanelState` data + `commit_input` + `commit_template_mode` +
   `commit_template_inputs` + `smart_commit` + `pending_smart_msg` + `app: WeakEntity<KagiApp>` +
   `repo_path: PathBuf` + a `gen: u64` (NEW — guards smart-commit async results, map flag).
2. **Parent-owned (do NOT move)**: `commit_panel_open` (graph-selection gate, set by `select`),
   `conflict_merge_pending` (already parent, Stage 0d), `file_menu` (shared overlay),
   `last_draft_value`/`draft_save_gen` (draft autosave debounced on the parent render cycle —
   map note #6), `smart_commit_detected_for` (per-repo run-once guard, like conflict `detected_for`).
3. **Render**: parent renders `el.child(commit_panel_entity.clone())` under the existing gate; the
   22 listeners + per-row fns retarget to `Context<CommitPanelView>` / `&CommitPanelView`. Element
   tree/styles/handlers byte-identical.
4. **Action dispatch (HARD INVARIANT, proven by ConflictView)**: no `CommitPanelView` listener
   synchronously calls a `KagiApp` method that reads/updates `app.commit_panel`. The backend actions
   (`start_commit`/`do_stage*`/`do_unstage*`/`discard_all`/`open_commit_plan_modal`, and
   stage/unstage which call `refresh_wip_diffstat` that reads `commit_panel`) DEFER via
   `cx.spawn_in(window, …)` + `weak_app.update_in(&mut acx, |app, window, cx| app.X(…))`. Pure
   entity-internal mutations (file select/highlight, tree↔flat toggle, template-mode toggle of the
   entity's own inputs, file_menu open which writes the parent `file_menu` — keep that one a
   deferred/parent write since file_menu is parent) stay synchronous + child `cx.notify()`.
5. **Stage/unstage**: `do_stage_file` does `repo.stage_file` + `panel.reload_status` (entity-internal)
   + `refresh_wip_diffstat` (parent, reads commit_panel → defer). Re-shape so the entity updates its
   own lists, then marshals the wip-diffstat refresh to the parent.
6. **smart-commit generation gains a `gen` guard** (bump on each generate, check at apply) so a
   stale LLM/CLI result can't clobber a newer input (map flag — current `overwrite_ok` check is racy).
7. **Cross-flow preserved**: `open_commit_panel` (parent) creates the entity via `cx.new` and sets
   its `commit_input` value; `conflict_continue` (parent, already deferred from the conflict entity)
   keeps driving it; `start_commit`/`finish_merge_commit` stay parent and check `conflict_merge_pending`.
8. **Behaviour-preserving** except any flagged delta. `[kagi]`/`klog!` lines byte-identical + order
   (commit-panel unstaged/staged counts ×3, plan/blockers, async commit started/finished/failed,
   executed: commit/merge commit, draft saved/cleared/loaded, smart-commit lines, staged/unstaged).

## Open questions for the adversarial cross-plan review
1. Does deferring `start_commit`/`finish_merge_commit`/stage/unstage fully avoid the re-lease, given
   the conflict→merge-commit cross-flow already drives the panel from the (deferred) conflict path?
   Trace `finish_merge_commit`'s `reload(cx)` (which rebuilds `commit_panel`) called from a
   CommitPanelView commit-button listener.
2. `commit_input`/`commit_template_inputs` are nested `Entity<InputState>` — any issue owning them
   inside `CommitPanelView` vs the parent (focus handling, `sync_modal_inputs`, IME)? Does the parent
   render-sync that pushes `pending_smart_msg`/draft into the input need to move into the entity?
3. Draft autosave reads the input each render and debounces via `draft_save_gen` — keep on parent
   (reading the entity's input each frame from the parent render = the re-entrancy-in-render hazard?)
   or move into the entity?
4. `commit_panel_open` vs `commit_panel.is_some()` — are they always consistent, or does the gate
   need both? Any path that sets one without the other?
5. Is the `gen` guard for smart-commit a behaviour change (could it drop a result that today lands)?
6. Sequencing: anything that makes this unsafe to do now that ConflictView is an entity (the
   conflict→commit-panel handoff crosses two entities via the parent)?

## Cross-review corrections (codex adversarial review — MUST apply)

1. **[Q3 — corrects design item 2] Draft autosave moves INTO the entity.** `last_draft_value` +
   `draft_save_gen` + the autosave logic (`sync_modal_inputs` reading `effective_commit_message`,
   `mod.rs:2712-2716`) must live on `CommitPanelView`, NOT the parent. Keeping them on the parent
   forces the parent render to read the child's input every frame = the re-entrancy-in-render surface
   ADR-0118 explicitly forbids (`0118:24`). Override the earlier "parent-owned draft" decision.
2. **[Q2] The parent render input-sync moves INTO the entity.** `render.rs:196`
   (`pending_smart_msg.take()` → `set_template_inputs` / `commit_input.update(set_value)`) and the
   draft-autosave half of `sync_modal_inputs` must run on the entity's own update/render path. Keep
   the `InputState` entities STABLE across status reloads (don't recreate) or IME/focus regresses.
3. **[Q1/Q6] Defer EVERY commit-button path.** The commit button → `open_commit_plan_modal`
   (`commit_panel_render.rs:831`) → may `start_commit` (`commit.rs:801`) → if `conflict_merge_pending`
   → `finish_merge_commit` (`commit.rs:860`) which mutates `commit_panel` + `reload(cx)`
   (`commit.rs:1012`, rebuilds the panel) — ALL must defer via `spawn_in`/`update_in`. `open_commit_panel`
   (parent) must create the NEW entity via `cx.new` and must NOT synchronously update the leased
   ConflictView during the merge handoff.
4. **[Q4] `commit_panel_open` ≠ `commit_panel.is_some()`.** `select()` (`mod.rs:3125`) sets
   `open=false` WITHOUT clearing `commit_panel`. Keep BOTH; invariant: open flag = visibility, entity
   presence = cached state. Render gate stays `commit_panel_open` (`render_body.rs:436`).
5. **[Q5 — non-preserving, call out] smart-commit `gen` guard.** Adding the guard can DROP a stale
   result that lands today (current `overwrite_ok` captured pre-gen, `commit.rs:491/536`). Intentional
   tightening — flag it in the PR.
6. **[missed risks] (a)** the virtualized per-row fns (`commit_panel_render.rs:39…`) read
   `&KagiApp`/`this.commit_panel` → become pure `&CommitPanelView` reads. **(b)** `discard.rs:19`
   (discard planning) reads `commit_panel`, and `file_menu` is a parent overlay → either move
   `file_menu` into the entity or DEFER every parent discard/menu open. **(c)**
   `refresh_working_tree_external` (`mod.rs:2084`) reloads an OPEN panel in place → after promotion it
   must `entity.update(...)`, never rebuild via a parent render read.

## Done = all green + verified (same gates as ConflictView)
- build + `test --workspace` (≥791/0) + fmt + no new clippy + `KAGI_COMMIT_PANEL=1` headless smoke.
- PM real-UI verification: open panel, stage/unstage, type message, commit (plain), amend, and the
  **conflict→resolve→Continue(merge)→commit panel→commit** cross-flow; no "already borrowed" panic.
- Adversarial codex cross-review of the diff before integrate.
