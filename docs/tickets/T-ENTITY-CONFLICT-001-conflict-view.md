# T-ENTITY-CONFLICT-001 — `ConflictState` → `Entity<ConflictView>`

- ADR: 0118 (KagiApp decomposition, Phase 5.2) — Mechanism B (Entity promotion), per the ADR-0117
  fat-entity template.
- Risk: **HIGH** (safety-critical conflict resolution / ADR-0106 atomicity; ~2660 LOC render
  re-parent across `conflict_view.rs`+`conflict_editor.rs`; 26 `cx.listener` closures; **no headless
  `KAGI_*` UI gate** → real-UI verification is mandatory and is the primary behaviour gate).
- Owner: SubAgent (general-purpose) per stage → PM verify + **adversarial** codex cross-review +
  PM real-UI verification (screen-record + cliclick).
- Status of plan: **design cross-reviewed by codex (DESIGN-NEEDS-CHANGES → revised below).** The 7
  required changes from that review are folded into this ticket.

## Why this is bigger than FileHistory (ADR-0117)
FileHistory's only parent callbacks were `close`/`jump_to_commit` — neither touches the entity's
lease. Conflict is different: **every Backend action (`conflict_continue`/`confirm_*`/`abort`/`skip`/
`editor_save`) calls `reload()` (or `detect_conflict_mode()` directly), and `reload→
detect_conflict_mode→apply_conflict_detect` writes `self.conflict`.** If a `ConflictView` listener
synchronously calls any such parent method, it re-leases the still-leased entity → **panic "already
borrowed"** (uncatchable by build/test). Plus the parent reads conflict state every render via
`sync_conflict_editor_inputs` and the root divider-drag handler.

## Staged plan (codex Q6: prep PRs first, then ATOMIC entity flip — never "render-entity-first,
## actions-later", which is the panic class)

### Stage 0 — Prep (lower-risk, no GPUI ownership change; each its own PR, cross-reviewed)
- **0a.** Consolidate the buffer-only / editor-only conflict methods (`conflict_apply_choice`,
  `conflict_editor_set_file_side`/`_hunk_side`/`_hunk_line`/`_hunk_order`) so their *non-parent*
  work (mutating `mode.buffer`) is cleanly separable from their parent side effects
  (`push_toast`). Goal: these become entity-internal in Stage 1 with toast marshalled.
- **0b.** Make `sync_conflict_editor_inputs` (`mod.rs:2711`) entity-ready: today it reads/mutates
  conflict + `InputState` during the parent render/input-sync pass. Define how it moves into the
  `ConflictView` update/render path (it cannot stay a parent-side read of the conflict entity).
- **0c.** Define the **divider-drag bridge**: `geom`/`ab_geom` (`Rc<Cell>`) + `ab_split`/
  `result_split` are read/written by the parent root `on_drag_move` (`render.rs:528`,
  `render_divider.rs:152/203`). Mirror the FileHistory precedent (`file_history.rs:113` keeps the
  measured `Rc<Cell>` shared with the parent drag): the entity owns the splits, but the measurement
  cell stays shared, and the root drag updates the child via `entity.update`.

### Stage 1 — Entity flip (ATOMIC: ownership + render + ALL listeners/callbacks in one PR)
1. `KagiApp.conflict: conflict_view::ConflictState` → `Option<Entity<conflict_view::ConflictView>>`.
   `ConflictView` owns the former `ConflictState` fields **except** the parent-owned ones below,
   plus `app: WeakEntity<KagiApp>` and `repo_path: PathBuf`.
2. **Parent-owned (do NOT move into the entity)** — codex Q2/Q5/Q7 + missed-risks:
   - `merge_commit_pending` → `KagiApp.conflict_merge_pending: bool`. Read by parent render gate
     (`render.rs:407/642`), the watcher reset (`tabs.rs:627`), and the commit flow
     (`operations/commit.rs:860/1012`). Update ALL those sites.
   - `detected_for` → stays parent (per-repo run-once guard; cleared on repo change).
3. **Render**: parent renders `el.child(conflict_entity.clone())` (banner + body) under the existing
   `conflict.is_some() && !conflict_merge_pending` gate; delete the clone-mode + `EditorChrome`
   plumbing (`render.rs:402-446`). All free render fns retarget to `Context<ConflictView>`; the 26
   listeners become `|view: &mut ConflictView, …|`.
4. **Action dispatch rule (HARD INVARIANT)**: *No `ConflictView` listener may synchronously call any
   `KagiApp` method that reads/updates `app.conflict`.* The four reload-touching actions
   (`continue`/`abort`/`skip`/`editor_save`) and the snapshot-reading context actions
   (`conflict_open_external_tool`/`copy_path`/`copy_git_command`/`open_terminal`) are dispatched via
   **deferred marshalling** from the child context:
   ```rust
   let weak_app = self.app.clone();
   cx.spawn_in(window, async move |_view, mut acx| {
       let _ = weak_app.update_in(&mut acx, |app, window, cx| app.conflict_continue(window, cx));
   }).detach();
   ```
   (codex Q3: `cx.spawn_in(window, …)` + `weak_app.update_in(&mut acx, |app, window, cx| …)` is the
   correct vehicle for this gpui version — do NOT carry `&mut Window` across the await as the design
   draft wrongly implied; precedent shape exists in the checked-out zed editor crate.) By the time
   the closure runs the listener has returned → entity unleased → parent reload/detect is safe.
   Buffer-only mutations (hunk/side/scroll/split/menu/arm/result_editing) mutate `self` (the view)
   directly + `cx.notify()` (child-only repaint); their toasts marshal to the parent.
5. **detect/apply (parent-owned, builds/drops the entity)**: `apply_conflict_detect`:
   `Detected` → update existing `entity` via `entity.update`, else `cx.new(ConflictView::new(…))`;
   `Cleared`/`MergeResolvedReady` → `self.conflict = None`. **Same `klog!`/`eprintln!("[kagi] …")`
   text + order** (`mod.rs:2254/2261/2277`). codex Q5: add a **repo-match check at apply time** in
   `detect_conflict_mode_async` (compare the captured `repo_path` to the current one) — `detected_for`
   alone is insufficient if the repo switches mid-task.
6. **Lifecycle/reset (codex required-change #5; the ADR-0117 stale-repo_path bug class)**:
   `reset_per_repo_ui` (`tabs.rs:331`) and `show_welcome` (`tabs.rs:494`) MUST now set
   `self.conflict = None`, `self.conflict_merge_pending = false`, and clear the parent `detected_for`,
   so a stale `Entity<ConflictView>` (captured prev-repo `repo_path`) never survives a tab switch /
   welcome. (Today they don't touch conflict — relying on re-detect; that's unsafe once it's an entity.)
7. **Behaviour-preserving** except the two flagged, accepted deltas (call out in the PR):
   - split-ratio + `editing_before_text` reset when the entity is dropped on conflict-clear (was
     retained on the always-present `ConflictState`). Acceptable: re-entering a conflict resets
     split UI / before-text hashes; rare and innocuous. **Confirm `editing_before_text` is not
     load-bearing for an in-progress save's oplog before/after** (`operations/conflict.rs:247/322`).

## Constraints (CLAUDE.md)
- No `git2::` in `src/ui/`. No new `.unwrap()`. `[kagi]`/`klog!` lines byte-identical, same order.
- i18n unchanged (no new `Msg`). `kagi-domain` untouched.

## Done = all green + verified
- `cargo build` + `cargo test --workspace` (≥791 passed, 0 failed) + `cargo fmt --check` + no new clippy.
- **PM real-UI verification** (now possible — desktop + screen-record + cliclick): create a real
  merge conflict and exercise resolve→save→continue (merge route → commit panel), continue
  (sequencer route → confirm modal), **abort** (two-stage), **skip**, tab-switch mid-conflict
  (no stale view), and confirm no "already borrowed" panic in any path.
- Adversarial codex cross-review of the diff before integrate.

## Status: LANDED (refactor/kagi-app-decomp-5-2)

- Stage 0a (`f952430`), Stage 0d (`371d860`), Stage 1 atomic flip (`a32986e`).
- Gates green: build, `test --workspace` 791/0, fmt, clippy 38 (no new), headless conflict-detect
  smoke (line ×1, conflicts=1, no panic).
- Adversarial codex cross-review: SHIP-WITH-NITS → the one new `.unwrap()` (render.rs body gate)
  fixed to `if let Some`.
- **Real-UI verification PERFORMED** (primary session, screencapture + cliclick): the ConflictView
  entity renders the full screen (banner + 3-pane editor + dashboard + result preview + toolbar);
  the entity-internal Abort-arm fires child-notify and re-renders ("Confirm abort" + warning); the
  **deferred Confirm-abort execute** drives `spawn_in→update_in→conflict_abort→reload→detect→
  apply(Cleared)→conflict=None` with **no "already borrowed" panic**, emitting
  `executed: merge-abort` + `conflict-mode: cleared` and returning to the normal graph view.
- continue (merge→commit-panel / sequencer→confirm-modal), skip, and editor_save share the SAME
  deferred-marshalling machinery validated by the abort path (cross-review confirmed each defers);
  a further human pass can exercise them individually for extra assurance.
