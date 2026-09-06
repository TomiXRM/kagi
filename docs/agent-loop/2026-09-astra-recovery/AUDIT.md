# Current-HEAD recovery audit

Baseline: `5f7a4c80618d270010a1360a4c4a062c75147be5`, 2026-09-06. Locations below refer to that baseline unless explicitly marked recovery. Historical reviews were hypotheses, not numerical evidence.

## Method and integration

Four independent read-only slices covered architecture/ownership, cache/performance, GPUI/layout and safety. Main re-read decisive boundaries, executed baseline checks, reproduced missing stash-drop persistence, equal-size moved-mark corruption and Table overflow. Speculative Card edits were rejected: source suspicion is not a reproduced defect. Full working evidence lives in `/tmp/kagi-astra-recovery-evidence/`; the relevant facts and inventories are preserved here and in METRICS.md so those temporary reports are not prerequisites for understanding the change.

`main` is the real default branch; no `master` exists. 114 local/tracking refs were checked by ancestry and patch equivalence, then squash/history review. The user explicitly deferred the genuinely unintegrated package-distribution branch. The original checkout, request.md and Claude's #476 worktree were not modified. `dev` starts at the audited main SHA; no blind merge, push, tag, release or authentication-dependent test was performed.

LSP could not start: rust-analyzer is absent for pinned 1.96.0. Lens structural activation rejected its documented schema. Main used pinned tree-sitter 0.24.0 / tree-sitter-rust 0.23.2 through Python 3.12/uv, with actual Rust AST nodes, plus compiler and CI checks. Counts include source-local cfg(test) nodes but exclude integration-test directories and comments/strings. `notify` counts distinguish `cx.notify()` from the unrelated git2 callback method. Expressions whose callee text starts with a qualified Git prefix are a bounded syntactic inventory, not distinct Git calls or an LSP-complete alias/method call graph.

## Actual workspace dependency DAG

Cargo metadata resolves **11 workspace packages**, including implicit path member gpui-terminal. Arrows show workspace dependencies only, including target-conditional edges; external GPUI/git2 dependencies are omitted here, not absent.

```text
gpui-terminal -> (no workspace dependency)
kagi -> gpui-terminal, kagi-domain, kagi-git, kagi-ui-core, kagi-ui-ecosystem, kagi-ui-editor, kagi-ui-file-history
kagi-domain -> (no workspace dependency)
kagi-git -> kagi-domain
kagi-mcp -> kagi-domain, kagi-git
kagi-ui-core -> kagi-domain
kagi-ui-ecosystem -> kagi-domain, kagi-ui-core
kagi-ui-editor -> kagi-domain, kagi-ui-core
kagi-ui-file-history -> kagi-domain, kagi-ui-core
kagi-web -> kagi-domain
xtask -> (no workspace dependency)
```

`kagi-domain/Cargo.toml` remains dependency-free. `kagi-git` is the git2/I/O owner. Root `kagi` still hosts UI, application orchestration and shell; there is no kagi-app/kagi-ui boundary preventing root UI from importing the facade. UI-core is not merely a dumping ground: commit-row rendering has real Editor/File-History consumers, and feature crates share theme/domain data without importing the root. Their load results cross explicit callback seams; no new crate is justified for this recovery.

## Hypotheses and selected priorities

| Hypothesis | Current evidence and decision |
|---|---|
| Root owns state, async, cache and view | Confirmed: KagiApp has 113 fields; snapshot application, `tabs.rs`, `reload.rs`, `workspace.rs`, `operations/*` all update it. Keep the large application-layer migration separate. |
| impl splitting is not ownership transfer | Confirmed: 64 inherent impl blocks in 51 files. `src/ui/render.rs:3-8` explicitly describes a physical split; `operations/mod.rs:1-10` similarly preserves methods on the same owner. |
| Feature crates are merely file relocation | Refuted as a blanket claim. UI-core's commit_row and theme have multiple real consumers; editor/file-history/ecosystem own entities and expose domain/callback seams. Root still coordinates their I/O. |
| Row-index cache keys force clears | Confirmed by design: `diff_cache.rs:21-54`, `tab_view.rs:250-256`, `reload.rs:71,148`, `tabs.rs:368,568`, `graph_solo.rs:63`. Existing clear-on-renumber prevents stale rows; replacing it requires immutable OID/path/options identity and asynchronous consumer migration, not deleting clear calls. |
| Unbounded and duplicate caches exist | Confirmed: full inventory below. Active TabViewState is cloned into tab_cache on reload; ecosystem data outlives closed tabs; several memory/disk maps lack eviction. No measured process-memory claim is made. |
| Fixed-height text still breaks | Main reproduced **Table** overflow: 320px parent, last column at x320..400 under 125% EN. Card risk from the initial source review was not reproduced; retain it only as a regression guard. Diff and oplog already use variable-height lists. |
| Modals lack shared visual grammar | Refuted as a blanket claim: `modal_shell.rs` already owns card/section/list geometry, tokens and viewport caps. Empty/loading helpers differ across feature crates, but merging them would be cosmetic cross-crate churn. |
| Old architecture plans imply already-completed work is pending | Confirmed: the old 'no crate separation' claim, old root sizes and T019 ZWSP plan are not current facts. Target kagi-app/kagi-ui topology remains a target, not implemented separation. |

Selected: P1 mutation-owned persistent completion; P2 one bounded split projection; P3 reproduced history-Table layout. A separately verified mechanical backend-recorder extraction precedes P1. No P0 safety guarantee was intentionally relaxed. New issues found while implementing are recorded, not hidden behind test changes.

## Safety and completion ownership

- Baseline stash-drop executed but produced **zero** durable records in the native UI reproduction. History moved refs through legacy executors without mutation-owned recording. A simple `record_op_persist` in an async callback would still lose records after a tab switch.
- Cleanup had a bespoke plain-await callback (`branch_cleanup.rs:260-380`), unlike the existing shared `finish_op_on_main` guard (`operations/mod.rs:95-163`). Recovery reuses that guard and releases the confirmed modal before dispatch.
- The backend is now the durable owner for stash drop, history and cleanup; remote stash drop records at its existing transport boundary. UI receives full recovery OIDs but only presents already-recorded results. An opening failure must record in the background caller because no backend exists yet.
- Review caught two intermediate regressions: generic execution labels replaced localized preflight labels; cleanup opening failures lost persistent records. Both were fixed and exercised in native fixture scenarios. Typed preflight errors preserve underlying Display and the old UI phase labels without double-preflight or string classification.
- Unchanged limitations: cleanup's primitive can lose the remote-half result if a subsequent local deletion fails (`ops/branch_cleanup.rs:481-486`); root Enter routing lacks standalone stash-drop (`mod.rs:3191-3276`). The new stash UI scenarios exercise its confirmation handler; the history refusal scenario exercises actual Enter. Neither gap is represented as fixed.
- Backend record append still has its existing failure policy; exactly-once successful appends are not a guarantee against disk-full/process failure. No new recovery schema is introduced.

## Full cache inventory at baseline

Weights are source-based estimates, not measured retained RSS. 'None' under instrumentation means unmeasured, not zero hits/misses. Only the selected split derivation is temporarily instrumented; other caches retain their established behavior.

Columns: Name | Owner | Key | Value | Est. weight | Lifetime | Invalidation | Capacity | Hit/miss instrumentation | Duplicate value elsewhere | Correctness risk.

| # | Name (file:line) | Owner | Key | Value | Est. weight | Lifetime | Invalidation | Cap | Hit/miss instr. | Duplicate | Risk |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | `DiffCaches.changed_files` (diff_cache.rs:21) | `KagiApp.diff_caches` (mod.rs:991) | commit ROW INDEX (usize) | `Option<Vec<FileStatus>>` (`None` = failed load) | ~0.1 KB x files per visited row | until next renumber | `DiffCaches::clear()` via `invalidate_caches_for_row_renumber` (tab_view.rs:250), reload (reload.rs:71,148), tab switch (tabs.rs:368), solo (graph_solo.rs:63), welcome (tabs.rs:568) | unbounded (rows visited between clears) | `[kagi] changed files: N` on fill only | commit-panel lists are a separate source | row-key staleness (#286) patched by clear-on-renumber; a failed load (`None`) is cached and never retried until next clear |
| 2 | `DiffCaches.file_content` (diff_cache.rs:26) | same | `(row, file_index)` | `Arc<FileDiff>` (full hunks + line text) | KBs–MBs per huge diff | same | same | unbounded | none | rows re-derived into `MainDiffView` (2nd representation in pane) | same clear-on-renumber protection; hit path diff_view.rs:1370–1394 |
| 3 | `DiffCaches.remote_inflight` / `local_inflight` (diff_cache.rs:29,33) | same | row index | `HashSet<usize>` guard | trivial | same | same + removed on completion (mod.rs:2483,2540) | unbounded | no | – | guards double-spawn; ok |
| 4 | `DiffCaches.diffstat` (diff_cache.rs:36) | same | row index | `Vec<FileDiffStat>` | ~24 B x files | same | same | unbounded | none | inspector consumes clone | same as #1 |
| 5 | `DiffCaches.generated` (diff_cache.rs:40) | same | row index | `Vec<bool>` | trivial | same | same | unbounded | none | – | same as #1 |
| 6 | `tab_cache` (mod.rs:1282) | KagiApp | repo `PathBuf` (synthetic `host:root` for remote) | full `TabViewState` (rows, details, 3 hashmaps, activity, cleanup rows) | ~1–10 MB per 10k-commit repo | until `close_tab` eviction (tabs.rs:513) | overwritten on every reload/tab-load (reload.rs:75,201; tabs.rs:455) | bounded by open tabs | `[kagi] tab-switch: <name> cached=yes|no` (tabs.rs:194) — the ONLY explicit hit/miss log in the app | **duplicates `active_view` for the active tab** (apply_tab_view moves a clone; reload inserts `view.clone()` then applies the original) | stale-while-revalidate by design; `switch_generation` guards stale loads |
| 7 | `AvatarStore.images` (avatar.rs:33; type kagi-ui-core/avatar.rs:20) | KagiApp.avatars | author email | `Arc<gpui::Image>` (encoded bytes, s=64) | ~2–10 KB each | process lifetime, **never evicted**, accumulates across repos | none | unbounded | `[kagi] avatar: resolved=N pending=M offline=` (avatar_resolve.rs:88) | disk cache (#9) | stale avatar until restart; weight negligible |
| 8 | `AvatarStore.attempted` + `scan_epoch` (avatar.rs:39,44) | same | email / view_epoch | guards | trivial | reset on repo change (avatar_resolve.rs:36) | – | unbounded per repo | same log | – | deferred emails removed for retry — correct |
| 9 | avatar disk cache (avatar_fetch.rs:176–228) | filesystem `~/.kagi/avatars` or `$KAGI_LOG_DIR/avatars` | FNV-1a(url) hex filename | raw image bytes | KBs each | cross-launch, **never GC'd** | none | unbounded | none | #7 | FNV collision = wrong avatar (documented, accepted) |
| 10 | `ecosystem_cache` (mod.rs:1421; type kagi-ui-ecosystem lib.rs:172) | KagiApp | repo `PathBuf` | `CachedMine{RawEcosystem, head}` (per-file churn/loc/coupling of whole repo) | MBs for big repos | session; **NOT evicted in `close_tab`** (tabs.rs:504–533 removes only terminal_sessions + tab_cache) | HEAD-move removal (reload.rs:363–372); reuse guard ecosystem.rs:31–37, 78–80 | unbounded per repos-opened | klog mine lines | none | HEAD-keyed, correct; memory retained for closed repos |
| 11 | `EditorWorkspaceView.tab_cache` (kagi-ui-editor lib.rs:322) | editor entity | file `PathBuf` | `EditorBufferState` (content String <=10 MB gate, own `InputState`, diff box) | up to 10 MB per open editor tab | entity lifetime (dropped on repo switch via dispose, workspace.rs:294) | tab close | bounded by open editor tabs | none | none | per-tab InputState is deliberate (undo isolation) |
| 12 | `EditorState.mermaid` (kagi-ui-editor lib.rs:494) | editor entity | u64 hash(content, dark) | `MermaidState` (path / rendering / failed) | trivial in memory | entity lifetime | none (insert-only, lib.rs:2041,2053) | unbounded count | `[kagi] editor-ws: mermaid rendered` | disk PNG cache (#13) | ok |
| 13 | mermaid disk cache (kagi-ui-editor markdown.rs:147–157) | filesystem | key.png/.mmd | PNG bytes | KBs–100s KB | cross-launch, **never GC'd** | none | unbounded | none | #12 | ok |
| 14 | `SIDE_CACHE` (conflict_binary_view.rs:116–122) | **thread_local static** | blob OID + kind string | `SideData` (may hold decoded `Arc<Image>` <= IMAGE_SIZE_CAP) | up to cap per side | **session lifetime, never evicted** (comment says "ponytail") | none | unbounded | none | none | content-addressed → no staleness; growth only with many binary conflicts; **miss does Backend::open + blob read inside render()** (line 255) |
| 15 | `SideHlCache` (conflict_editor.rs:391–441) | ConflictView chrome (`RefCell<Option<_>>`) | (path, theme slug) | both sides' highlight spans (`Arc<Vec<RowHl>>`) | ~rows x spans | single entry | replaced on key change | 1 | none | none | assumes side text immutable per file/session (true: selection only flips `taken`); ConflictView dropped on reload |
| 16 | `MOVED_CACHE` (diff_split.rs:73) | **process static Mutex** | `surface_key` = hash(title, row_count) (diff_selection.rs:33) | `Arc<HashSet<usize>>` moved rows | ~rows | process | FIFO evict at 8 (diff_split.rs:82–85) | 8 entries, no byte budget | none | none | **key collision risk**: two diffs w/ same file name AND same row count share one entry → wrong moved-block marks (unlike SELECTION where collision is explicitly harmless) |
| 17 | `SELECTION` (diff_selection.rs:28) | process static | surface_key | 1 selection + extracted text | small | until Esc/clear | clear() | 1 | none | none | text stored eagerly — collision cannot copy wrong text (documented) |
| 18 | `ConflictViewState.editing_before_text` (conflict_view.rs:129) | ConflictView | path | pre-edit file String | file-sized | conflict session | entity drop | conflicted-file count | none | none | ok |
| 19 | CommitPanel derived caches (commit_panel.rs:89–96: `unstaged_tree`, `staged_tree`, `*_stat_index`, fold index lists) | CommitPanelView | – | trees + path→idx maps | ~files | until `reload_status` | rebuilt per status change (`rebuild_derived`, commit_panel.rs:241) | bounded by WT files | none | none | explicit render-perf caches (was O(N²)/frame); correct |
| 20 | `PrTab` per-tab data (pr_mode.rs:44–90: commits, files, diff, reviews/comments, `conflicts`, `conflict_text`, `merge_status`) | `pr_mode.tabs` | tab | vectors + built diff | 50 MB lesson led to per-selection `conflict_text` | until tab close / repo switch (dispose workspace.rs:193) | conflicts computed once/tab (pr_mode.rs:445–449) | bounded by open PR tabs | klog pr-mode lines | diff duplicates #2 representation for PR surface | ok |
| 21 | `sidebar.rows` + `rows_fingerprint` (render.rs:377–402) | KagiApp.sidebar | fingerprint(view_epoch, prs_epoch, lengths, collapsed, filter) | flattened row list | ~refs | frame-to-frame | fingerprint mismatch rebuild | 1 | none | derived from active_view | epoch-driven; correct |
| 22 | `ruleset` cache (kagi-git ruleset.rs:38–55) | **process static Mutex** | `workdir\0branch` | `RulesetStatus` | small | process, **never invalidated** | none | unbounded | unit-tested seam, no runtime counter | none | stale if GitHub ruleset changes mid-session (accepted, avoids rate limit) |
| 23 | `RepoSession` (kagi-git session.rs:26–53; ui/tabs.rs:149,177) | active-local-tab slot `KagiApp.repo_session` | – | `Rc<Backend>` (open repo handle) | one read handle per retained session; worker lazy | until replacement/clear on switch; existing Rc clones may outlive it | newly opened on local switch, cleared on remote switch | one retained slot per KagiApp, not one per open tab | none | – | avoids per-interaction open while active; reopens on tab return; source comments overstate tab-lifetime retention |
| 24 | `terminal_sessions` (mod.rs:1136) | KagiApp | repo path | live PTY session | OS pty | until `close_tab` eviction (tabs.rs:511) | eviction | bounded by tabs | none | none | ok |
| 25 | OnceLock constants: gh availability/version (kagi-git github.rs:24, github_merge.rs:46), claude PATH (message_gen.rs:565), EUID probe (trust.rs:106), terminal font (terminal.rs:322), command overrides (commands.rs:1123) | statics | – | small | trivial | process | none | 1 | none | none | stale only if env changes mid-session (trivial) |
| 26 | `github_prs` + `github_prs_epoch/for` (mod.rs:1226–1240) | KagiApp | repo | PR list | small | per repo; cleared in `reset_per_repo_ui` (tabs.rs:363) | 60 s ticker + explicit refresh | bounded | klog | none | repo-stamped (`github_prs_for`) — guarded |
| 27 | `FileHistoryView.avatars` (kagi-ui-file-history lib.rs:176) | FH entity | – | full copy of #7 | = #7 | per frame refresh | overwritten **every frame** (workspace.rs:117) | – | none | **whole-map clone of #7 per frame** | see clones section |


Recovery delta: inventory #16 becomes a single weak-allocation-keyed projection cache holding paired rows and moved marks, with 8 FIFO entries and an 8 MiB conservative allocation budget. Dead sources are pruned; oversized values are returned but not retained. The budget excludes live renderers and does not retain source row buffers. #20 replaces one loaded raw marker string with one derived conflict preview per PR tab; repaint shares its rows/jumps. File/input replacement and theme/locale changes invalidate through copy-on-write; old snapshots remain immutable. This is replacement, not another cache layer. Graph-row DiffCaches are intentionally unchanged.

## Existing UI conventions

| Surface | Canonical implementation | Key styles |
|---|---|---|
| Modal card | modal_shell.rs `modal_card`/`modal_card_sized` :89-135 | bg `theme().panel`, border `panel_style()` hairline, `rounded_lg p_4 gap_3`, w ∈ {504,576,648}×zoom, max_w 90%, max_h 80%, overflow_hidden |
| Modal section | `modal_section`/`modal_section_chipped` :272-310 + `SectionOpen` type (bool-misuse made unrepresentable) | inset panel `panel_style()`, caret header, count chip always visible |
| Modal list | `modal_list_max_h` :56-66 | 18px rows × zoom, cap 40% viewport, floor 3 rows; prose: 25% viewport, floor 34px (`modal_prose_box`) |
| List row (files) | `modal_file_row` :443-520 | fixed 18px, A/M/D/R/T letter in `change_*` tokens, dir muted `text_ellipsis_start`, name `text_ellipsis`, MONO_FONT |
| Badge/chip | theme.rs `badge_style` (:~218): fill color@0x33, border @0x66, text white(dark)/text_main(light); `modal_chip` (px_2 rounded_full text_xs) | same grammar graph pills / modal chips / provenance badges |
| Buttons | button_style.rs `KagiButton::accent`/`apply_accent` | success/warning/blocker→theme-tinted custom; color_branch→`.primary()`; color_remote→`.info()`; else `.ghost()` |
| Row selection/zebra | render_helpers.rs:74-80 (bg_base/bg_row_alt/selected) ; commit_row.rs `row_background` :123-131 (panel/bg_row_alt/selected — panel-context zebra) | hover = `surface` tint, never `selected` (commit_row.rs comment :146-152) |
| Selected accent | 2px absolute left bar `color_branch` (render_helpers.rs:333-345) — absolute so zoom never shifts lane origin |
| Loading | render_body.rs:572 `render_loading_placeholder` (animated tri-dot, reduce_motion-aware, ADR-0165/0173) |
| Empty/loading (editor crate) | kagi-ui-editor lib.rs:2950 `placeholder_text` (centered single line) |
| Empty (ecosystem crate) | kagi-ui-ecosystem render.rs:526 `centered` |
| Empty (inline) | plain muted div (pr_mode.rs:1044-1050, branch_cleanup.rs:749-753) |
| Zoom | ALL fixed dims through `theme::scaled_px` / `scaled`; text through rem (`set_rem_size` render.rs:211); tailwind `px_3` etc. deliberately NOT zoom-scaled (render_helpers.rs:337-360 comment) |
| Text safety | `safe_text` (render_helpers.rs:27, control-byte escaping; unit-tested badges.rs:575-585); markdown via `sanitize_markdown_for_view` + `pad_inline_code` + `flatten_html_blocks` (pr_conversation.rs:18-131) |


Error modals and failure oplog/toast paths remain operation-owned; disabled controls remain GPUI Button state rather than a new styling system. Selection uses the existing selected token, hover the existing surface token, and focus the existing root/element focus handle. No literal colors or independent spacing/radius convention were added. Three-line diff ListState constructors remain duplicated across crate boundaries deliberately: removing that tiny duplication does not justify a dependency or policy abstraction. PR overdraw tuning is not mixed into the cache correctness fix.

## Current structural inventory

Baseline AST: 113 KagiApp fields; 64 inherent impls in 51 files; 443 `cx.notify()` calls (444 methods named notify, including one git callback). Lexical impl ownership is listed below; free-function callbacks are not falsely attributed to KagiApp. These are static callsites, not counts of runtime repaint events.

| Lexical owner | cx.notify sites |
|---|---:|
| <free function> | 152 |
| CommitPanelView | 6 |
| ConflictView | 2 |
| EcosystemView | 10 |
| EditorWorkspaceView | 29 |
| FileHistoryView | 4 |
| KagiApp | 234 |
| MainDiffPane | 3 |
| ToastStack | 3 |

### Inherent KagiApp impl distribution

| File | Blocks |
|---|---:|
| `src/ui/avatar_resolve.rs` | 1 |
| `src/ui/branch_cleanup.rs` | 2 |
| `src/ui/command_palette.rs` | 1 |
| `src/ui/commands.rs` | 1 |
| `src/ui/compare_pane.rs` | 1 |
| `src/ui/diff_view.rs` | 3 |
| `src/ui/ecosystem.rs` | 1 |
| `src/ui/editor_workspace.rs` | 1 |
| `src/ui/external_editor.rs` | 2 |
| `src/ui/file_history.rs` | 1 |
| `src/ui/github.rs` | 2 |
| `src/ui/graph_solo.rs` | 1 |
| `src/ui/graph_squash.rs` | 1 |
| `src/ui/main_diff_pane.rs` | 1 |
| `src/ui/mod.rs` | 2 |
| `src/ui/operations/branch.rs` | 2 |
| `src/ui/operations/checkout.rs` | 1 |
| `src/ui/operations/cherry_revert.rs` | 1 |
| `src/ui/operations/commit.rs` | 2 |
| `src/ui/operations/conflict.rs` | 2 |
| `src/ui/operations/discard.rs` | 1 |
| `src/ui/operations/editor_fs.rs` | 1 |
| `src/ui/operations/force_lease.rs` | 1 |
| `src/ui/operations/history.rs` | 1 |
| `src/ui/operations/mod.rs` | 1 |
| `src/ui/operations/modal_state.rs` | 2 |
| `src/ui/operations/pull_push.rs` | 1 |
| `src/ui/operations/rebase.rs` | 1 |
| `src/ui/operations/remote_branch.rs` | 3 |
| `src/ui/operations/reset.rs` | 1 |
| `src/ui/operations/stash.rs` | 1 |
| `src/ui/operations/tag.rs` | 2 |
| `src/ui/operations/worktree.rs` | 1 |
| `src/ui/platform_menu.rs` | 1 |
| `src/ui/pr_mode.rs` | 1 |
| `src/ui/reload.rs` | 1 |
| `src/ui/remote_browse.rs` | 1 |
| `src/ui/render.rs` | 1 |
| `src/ui/render_body.rs` | 1 |
| `src/ui/render_bottom.rs` | 1 |
| `src/ui/render_divider.rs` | 1 |
| `src/ui/render_header.rs` | 1 |
| `src/ui/render_overlay.rs` | 1 |
| `src/ui/render_status.rs` | 1 |
| `src/ui/render_wip.rs` | 1 |
| `src/ui/settings_view.rs` | 1 |
| `src/ui/tab_view.rs` | 1 |
| `src/ui/tabs.rs` | 1 |
| `src/ui/trust_prompt.rs` | 1 |
| `src/ui/workspace_mode.rs` | 1 |
| `src/ui/worktree_wip.rs` | 1 |

### Files above 800 lines

All 50 tracked Rust files in the baseline inventory; 41 are source paths (src/crates/xtask excluding tests directories). Ratchets remain the checked-in ci/loc-baseline.txt, never these historical counts.

| Path | LOC |
|---|---:|
| `src/ui/mod.rs` | 4233 |
| `crates/kagi-ui-editor/src/lib.rs` | 3346 |
| `crates/kagi-git/src/conflicts.rs` | 2642 |
| `crates/kagi-ui-core/src/i18n/mod.rs` | 2565 |
| `src/ui/pr_mode.rs` | 2349 |
| `crates/kagi-ui-core/src/theme.rs` | 2231 |
| `src/ui/commands.rs` | 2115 |
| `src/ui/conflict_view.rs` | 2073 |
| `crates/kagi-git/src/backend.rs` | 2041 |
| `src/ui/sidebar.rs` | 2007 |
| `tests/ops_test.rs` | 1826 |
| `src/ui/operations/branch.rs` | 1756 |
| `tests/conflicts_test.rs` | 1622 |
| `src/ui/diff_view.rs` | 1606 |
| `crates/kagi-git/src/resolution.rs` | 1537 |
| `src/ui/commit_panel_render.rs` | 1473 |
| `src/ui/conflict_editor.rs` | 1328 |
| `vendor/gpui-terminal/src/view.rs` | 1315 |
| `src/ui/operations/commit.rs` | 1300 |
| `src/ui/inspector.rs` | 1244 |
| `crates/kagi-git/src/ops/history.rs` | 1221 |
| `src/ui/tabs.rs` | 1168 |
| `src/ui/operations/modal_state.rs` | 1090 |
| `src/ui/branch_cleanup.rs` | 1064 |
| `src/ui/workspace.rs` | 1054 |
| `vendor/gpui-terminal/src/mouse.rs` | 1045 |
| `tests/worktree_test.rs` | 1043 |
| `crates/kagi-domain/src/resolution.rs` | 997 |
| `src/ui/blocking_ops.rs` | 995 |
| `src/ui/operations/conflict.rs` | 992 |
| `crates/kagi-git/src/ops/cherry_revert.rs` | 978 |
| `src/ui/branch_menu.rs` | 965 |
| `crates/kagi-git/src/ops/stash.rs` | 958 |
| `tests/graph_layout_test.rs` | 941 |
| `crates/kagi-git/src/staging.rs` | 934 |
| `tests/gui_e2e_runner.rs` | 927 |
| `tests/staging_test.rs` | 919 |
| `tests/discard_test.rs` | 917 |
| `src/ui/render.rs` | 914 |
| `crates/kagi-domain/src/message_gen.rs` | 895 |
| `crates/kagi-git/src/ops/worktree_steps.rs` | 876 |
| `src/ui/operations/worktree.rs` | 865 |
| `src/ui/commit_list.rs` | 838 |
| `src/ui/settings_view.rs` | 823 |
| `crates/kagi-git/src/ops/pull.rs` | 821 |
| `crates/kagi-git/src/ops/branch.rs` | 813 |
| `crates/kagi-git/src/ops/push.rs` | 808 |
| `crates/kagi-git/src/message_gen.rs` | 807 |
| `src/ui/operations/stash.rs` | 807 |
| `src/ui/render_header.rs` | 807 |

### Virtualized-list construction sites

10 uniform-list constructions (one conflict-side constructor mounts twice); 2 variable-list constructions plus 5 ListState constructors. These are different counts, not 7 variable list widgets.

- `crates/kagi-ui-editor/src/lib.rs:2364` — uniform_list
- `crates/kagi-ui-editor/src/panes.rs:169` — uniform_list
- `src/ui/branch_cleanup.rs:761` — uniform_list
- `src/ui/commit_panel_render.rs:1342` — uniform_list
- `src/ui/commit_panel_render.rs:1390` — uniform_list
- `src/ui/conflict_editor.rs:639` — uniform_list
- `src/ui/modal_renderers_destructive.rs:154` — uniform_list
- `src/ui/modal_renderers_destructive.rs:407` — uniform_list
- `src/ui/render_body.rs:385` — uniform_list
- `src/ui/sidebar.rs:1652` — uniform_list
- `crates/kagi-ui-editor/src/lib.rs:2991` — gpui::ListState::new
- `src/ui/oplog_panel.rs:42` — gpui::ListState::new
- `src/ui/oplog_render.rs:85` — gpui::list
- `src/ui/pr_mode.rs:229` — ListState::new
- `src/ui/pr_mode.rs:237` — ListState::new
- `src/ui/render_helpers.rs:489` — gpui::ListState::new
- `src/ui/render_helpers.rs:706` — gpui::list

### UI Git dependency sites

176 AST call expressions whose callee text starts with a qualified Git prefix, and 59 imports. The expression count includes chained `ok`/`map_err`/`and_then` wrappers as well as their inner Git calls (examples below); it is not a count of distinct Git operations. Imported aliases, receiver methods and callback wiring require the source audit above; this list does not pretend to resolve them without LSP.

- `src/ui/avatar_fetch.rs:161` — `kagi_git::Backend::open(repo_path).ok`
- `src/ui/avatar_fetch.rs:161` — `kagi_git::Backend::open`
- `src/ui/blocking_ops.rs:21` — `kagi_git::Backend::open`
- `src/ui/blocking_ops.rs:909` — `kagi_git::Backend::open`
- `src/ui/blocking_ops.rs:929` — `kagi_git::worktree_ports::assign_block`
- `src/ui/branch_cleanup.rs:125` — `kagi_git::Backend::open(&bg_path).map_err`
- `src/ui/branch_cleanup.rs:125` — `kagi_git::Backend::open`
- `src/ui/branch_cleanup.rs:133` — `kagi_git::github::gh_available`
- `src/ui/branch_cleanup.rs:290` — `kagi_git::Backend::open(&bg_path)
                .and_then`
- `src/ui/branch_cleanup.rs:290` — `kagi_git::Backend::open`
- `src/ui/commands.rs:1350` — `kagi_git::Backend::open(&repo_path).and_then`
- `src/ui/commands.rs:1350` — `kagi_git::Backend::open`
- `src/ui/commands.rs:1697` — `kagi_git::Backend::open(&repo_path).map_err`
- `src/ui/commands.rs:1697` — `kagi_git::Backend::open`
- `src/ui/commit_panel.rs:515` — `kagi_git::join_title_body`
- `src/ui/commit_panel.rs:531` — `kagi_git::join_title_body`
- `src/ui/commit_panel.rs:662` — `kagi_git::split_title_body`
- `src/ui/commit_panel.rs:694` — `kagi_git::clear_draft`
- `src/ui/commit_panel.rs:696` — `kagi_git::save_draft`
- `src/ui/conflict_binary_view.rs:256` — `kagi_git::Backend::open(mode.buffer.repo_path()).ok`
- `src/ui/conflict_binary_view.rs:256` — `kagi_git::Backend::open`
- `src/ui/conflict_binary_view.rs:444` — `kagi_git::Backend::open`
- `src/ui/conflict_view.rs:441` — `kagi_git::conflicts::side_labels`
- `src/ui/conflict_view.rs:553` — `kagi_git::Backend::open`
- `src/ui/conflict_view.rs:1970` — `kagi_git::Backend::open(repo_path).unwrap`
- `src/ui/conflict_view.rs:1970` — `kagi_git::Backend::open`
- `src/ui/diff_view.rs:1222` — `kagi_git::Backend::open`
- `src/ui/e2e.rs:62` — `kagi_git::open_repository(repo_path).map_err`
- `src/ui/e2e.rs:62` — `kagi_git::open_repository`
- `src/ui/e2e.rs:63` — `kagi_git::Backend::open(repo_path).map_err`
- `src/ui/e2e.rs:63` — `kagi_git::Backend::open`
- `src/ui/e2e.rs:70` — `kagi_git::session::RepoSession::open(repo_path).ok`
- `src/ui/e2e.rs:70` — `kagi_git::session::RepoSession::open`
- `src/ui/e2e.rs:115` — `kagi_git::oplog::OpLogEntry::new`
- `src/ui/ecosystem.rs:95` — `kagi_git::Backend::open(&bg_path)
                .map_err(|e| e.to_string())
                .and_then`
- `src/ui/ecosystem.rs:95` — `kagi_git::Backend::open(&bg_path)
                .map_err`
- `src/ui/ecosystem.rs:95` — `kagi_git::Backend::open`
- `src/ui/editor_workspace.rs:195` — `kagi_git::Backend::open(&repo_path)
                .ok()
                .and_then`
- `src/ui/editor_workspace.rs:195` — `kagi_git::Backend::open(&repo_path)
                .ok`
- `src/ui/editor_workspace.rs:195` — `kagi_git::Backend::open`
- `src/ui/editor_workspace.rs:221` — `kagi_git::Backend::open(&repo_path).map_err`
- `src/ui/editor_workspace.rs:221` — `kagi_git::Backend::open`
- `src/ui/editor_workspace.rs:255` — `kagi_git::Backend::open(&repo_path).ok().and_then`
- `src/ui/editor_workspace.rs:255` — `kagi_git::Backend::open(&repo_path).ok`
- `src/ui/editor_workspace.rs:255` — `kagi_git::Backend::open`
- `src/ui/editor_workspace.rs:302` — `kagi_git::file_history(&req).map_err`
- `src/ui/editor_workspace.rs:302` — `kagi_git::file_history`
- `src/ui/editor_workspace.rs:388` — `kagi_git::CommitId`
- `src/ui/editor_workspace.rs:389` — `kagi_git::Backend::open(&repo_path).ok().and_then`
- `src/ui/editor_workspace.rs:389` — `kagi_git::Backend::open(&repo_path).ok`
- `src/ui/editor_workspace.rs:389` — `kagi_git::Backend::open`
- `src/ui/file_history.rs:86` — `kagi_git::Backend::open(repo_path).ok`
- `src/ui/file_history.rs:86` — `kagi_git::Backend::open`
- `src/ui/file_history.rs:220` — `kagi_git::file_history`
- `src/ui/github.rs:29` — `kagi_git::github::gh_available`
- `src/ui/github.rs:36` — `kagi_git::github::list_open_prs`
- `src/ui/github.rs:86` — `kagi_git::github::gh_available`
- `src/ui/github.rs:96` — `kagi_git::github::current_login`
- `src/ui/github.rs:235` — `kagi_git::github::plan_pr_merge`
- `src/ui/github.rs:304` — `kagi_git::github::merge_pr`
- `src/ui/graph_squash.rs:167` — `kagi_git::Backend::open(&bg_path).and_then`
- `src/ui/graph_squash.rs:167` — `kagi_git::Backend::open`
- `src/ui/mod.rs:1535` — `kagi_git::OperationHistory::new`
- `src/ui/mod.rs:1922` — `kagi_git::Backend::open(&repo_path).ok`
- `src/ui/mod.rs:1922` — `kagi_git::Backend::open`
- `src/ui/mod.rs:2527` — `kagi_git::Backend::open(&repo_path).ok`
- `src/ui/mod.rs:2527` — `kagi_git::Backend::open`
- `src/ui/mod.rs:2528` — `kagi_git::CommitId`
- `src/ui/mod.rs:2590` — `kagi_git::Backend::open(repo_path).ok`
- `src/ui/mod.rs:2590` — `kagi_git::Backend::open`
- `src/ui/mod.rs:2628` — `kagi_git::Backend::open(&repo_path)
                .ok()
                .map`
- `src/ui/mod.rs:2628` — `kagi_git::Backend::open(&repo_path)
                .ok`
- `src/ui/mod.rs:2628` — `kagi_git::Backend::open`
- `src/ui/mod.rs:2734` — `kagi_git::Backend::open`
- `src/ui/mod.rs:3005` — `kagi_git::Backend::open(repo_path).ok`
- `src/ui/mod.rs:3005` — `kagi_git::Backend::open`
- `src/ui/modal_renderers_plan.rs:380` — `kagi_git::ops::PlanNote::Common`
- `src/ui/operations/branch.rs:133` — `kagi_git::Backend::open`
- `src/ui/operations/branch.rs:184` — `kagi_git::Backend::open`
- `src/ui/operations/branch.rs:707` — `kagi_git::Backend::open(&bg_path)
                .map_err`
- `src/ui/operations/branch.rs:707` — `kagi_git::Backend::open`
- `src/ui/operations/branch.rs:818` — `kagi_git::Backend::open(&bg_path)
                .map_err`
- `src/ui/operations/branch.rs:818` — `kagi_git::Backend::open`
- `src/ui/operations/branch.rs:1311` — `kagi_git::Backend::open(&bg_path).map_err`
- `src/ui/operations/branch.rs:1311` — `kagi_git::Backend::open`
- `src/ui/operations/branch.rs:1378` — `kagi_git::Backend::open`
- `src/ui/operations/checkout.rs:134` — `kagi_git::Backend::open`
- `src/ui/operations/checkout.rs:256` — `kagi_git::Backend::open`
- `src/ui/operations/checkout.rs:338` — `kagi_git::Backend::open`
- `src/ui/operations/checkout.rs:568` — `kagi_git::ops::PlanNote::Common`
- `src/ui/operations/commit.rs:188` — `kagi_git::Backend::open(&repo_path)
                .ok()
                .and_then`
- `src/ui/operations/commit.rs:188` — `kagi_git::Backend::open(&repo_path)
                .ok`
- `src/ui/operations/commit.rs:188` — `kagi_git::Backend::open`
- `src/ui/operations/commit.rs:203` — `kagi_git::load_draft`
- `src/ui/operations/commit.rs:205` — `kagi_git::split_title_body`
- `src/ui/operations/commit.rs:327` — `kagi_git::split_title_body`
- `src/ui/operations/commit.rs:514` — `kagi_git::Backend::open`
- `src/ui/operations/commit.rs:988` — `kagi_git::clear_draft`
- `src/ui/operations/commit.rs:1059` — `kagi_git::Backend::open`
- `src/ui/operations/commit.rs:1091` — `kagi_git::ResolutionBuffer::clear`
- `src/ui/operations/commit.rs:1093` — `kagi_git::clear_draft`
- `src/ui/operations/conflict.rs:329` — `kagi_git::split_title_body`
- `src/ui/operations/conflict.rs:358` — `kagi_git::ResolutionBuffer::clear`
- `src/ui/operations/conflict.rs:439` — `kagi_git::ResolutionBuffer::clear`
- `src/ui/operations/conflict.rs:801` — `kagi_git::Backend::open`
- `src/ui/operations/conflict.rs:826` — `kagi_git::ResolutionBuffer::new`
- `src/ui/operations/conflict.rs:848` — `kagi_git::resolve_selected_file`
- `src/ui/operations/discard.rs:248` — `kagi_git::Backend::open`
- `src/ui/operations/force_lease.rs:149` — `kagi_git::Backend::open(repo_path).map_err`
- `src/ui/operations/force_lease.rs:149` — `kagi_git::Backend::open`
- `src/ui/operations/history.rs:44` — `kagi_git::Backend::open(&repo_path)
                .map_err(|e| e.to_string())
                .and_then`
- `src/ui/operations/history.rs:44` — `kagi_git::Backend::open(&repo_path)
                .map_err`
- `src/ui/operations/history.rs:44` — `kagi_git::Backend::open`
- `src/ui/operations/history.rs:76` — `kagi_git::OperationHistory::seeded`
- `src/ui/operations/history.rs:437` — `kagi_git::Backend::open`
- `src/ui/operations/pull_push.rs:43` — `kagi_git::plan_pull_remote`
- `src/ui/operations/rebase.rs:164` — `kagi_git::Backend::open(repo_path).map_err`
- `src/ui/operations/rebase.rs:164` — `kagi_git::Backend::open`
- `src/ui/operations/remote_branch.rs:174` — `kagi_git::Backend::open(&bg_path)
                .map_err`
- `src/ui/operations/remote_branch.rs:174` — `kagi_git::Backend::open`
- `src/ui/operations/reset.rs:161` — `kagi_git::Backend::open(repo_path).map_err`
- `src/ui/operations/reset.rs:161` — `kagi_git::Backend::open`
- `src/ui/operations/stash.rs:45` — `kagi_git::Backend::open`
- `src/ui/operations/stash.rs:169` — `kagi_git::Backend::open`
- `src/ui/operations/stash.rs:233` — `kagi_git::Backend::open`
- `src/ui/operations/stash.rs:278` — `kagi_git::Backend::open`
- `src/ui/operations/stash.rs:344` — `kagi_git::Backend::open`
- `src/ui/operations/stash.rs:394` — `kagi_git::plan_stash_drop_remote`
- `src/ui/operations/stash.rs:407` — `kagi_git::Backend::open`
- `src/ui/operations/tag.rs:95` — `kagi_git::Backend::open`
- `src/ui/operations/tag.rs:276` — `kagi_git::Backend::open(&bg_path)
                    .map_err`
- `src/ui/operations/tag.rs:276` — `kagi_git::Backend::open`
- `src/ui/operations/worktree.rs:176` — `kagi_git::ops::plan_requires_worktree_trust`
- `src/ui/operations/worktree.rs:180` — `kagi_git::ops::plan_worktree_config_sha(&plan).unwrap_or`
- `src/ui/operations/worktree.rs:180` — `kagi_git::ops::plan_worktree_config_sha`
- `src/ui/operations/worktree.rs:181` — `kagi_git::ops::trust_worktree_config_at`
- `src/ui/operations/worktree.rs:314` — `kagi_git::Backend::open`
- `src/ui/operations/worktree.rs:402` — `kagi_git::ops::plan_requires_worktree_trust`
- `src/ui/operations/worktree.rs:404` — `kagi_git::ops::plan_worktree_config_sha(&modal.plan).unwrap_or`
- `src/ui/operations/worktree.rs:404` — `kagi_git::ops::plan_worktree_config_sha`
- `src/ui/operations/worktree.rs:437` — `kagi_git::Backend::open(&bg_path)
                .map_err`
- `src/ui/operations/worktree.rs:437` — `kagi_git::Backend::open`
- `src/ui/operations/worktree.rs:762` — `kagi_git::Backend::open`
- `src/ui/operations/worktree.rs:820` — `kagi_git::Backend::open`
- `src/ui/pr_mode.rs:269` — `kagi_git::github::pr_conversation`
- `src/ui/pr_mode.rs:270` — `kagi_git::github::pr_review_comments`
- `src/ui/pr_mode.rs:274` — `kagi_git::github::pr_merge_status(&repo2, number).ok`
- `src/ui/pr_mode.rs:274` — `kagi_git::github::pr_merge_status`
- `src/ui/pr_mode.rs:408` — `kagi_git::Backend::open(&repo_path).ok`
- `src/ui/pr_mode.rs:408` — `kagi_git::Backend::open`
- `src/ui/pr_mode.rs:466` — `kagi_git::Backend::open(&repo_path).map_err`
- `src/ui/pr_mode.rs:466` — `kagi_git::Backend::open`
- `src/ui/reload.rs:50` — `kagi_git::Backend::open`
- `src/ui/reload.rs:304` — `kagi_git::Backend::open`
- `src/ui/reload.rs:482` — `kagi_git::Backend::open(&bg_path).ok`
- `src/ui/reload.rs:482` — `kagi_git::Backend::open`
- `src/ui/reload.rs:583` — `kagi_git::Backend::open(repo_path).map_err`
- `src/ui/reload.rs:583` — `kagi_git::Backend::open`
- `src/ui/render_header.rs:191` — `kagi_git::github::gh_available`
- `src/ui/sidebar.rs:1607` — `kagi_git::github::gh_available().then`
- `src/ui/sidebar.rs:1607` — `kagi_git::github::gh_available`
- `src/ui/tabs.rs:177` — `kagi_git::session::RepoSession::open(&tab.path).ok`
- `src/ui/tabs.rs:177` — `kagi_git::session::RepoSession::open`
- `src/ui/tabs.rs:406` — `kagi_git::OperationHistory::new`
- `src/ui/tabs.rs:426` — `kagi_git::Backend::open(&bg_path)
                .map_err`
- `src/ui/tabs.rs:426` — `kagi_git::Backend::open`
- `src/ui/tabs.rs:1141` — `kagi_git::open_repository`
- `src/ui/tabs.rs:1163` — `kagi_git::session::RepoSession::open(&app.tabs[active].path).ok`
- `src/ui/tabs.rs:1163` — `kagi_git::session::RepoSession::open`
- `src/ui/trust_prompt.rs:52` — `kagi_git::trust::trust_repo`
- `src/ui/trust_prompt.rs:55` — `kagi_git::session::RepoSession::open(&modal.repo_path).ok`
- `src/ui/trust_prompt.rs:55` — `kagi_git::session::RepoSession::open`
- `src/ui/worktree_wip.rs:106` — `kagi_git::Backend::open`
- `src/ui/worktree_wip.rs:126` — `kagi_git::Backend::open(path)
            .ok()
            .and_then`
- `src/ui/worktree_wip.rs:126` — `kagi_git::Backend::open(path)
            .ok`
- `src/ui/worktree_wip.rs:126` — `kagi_git::Backend::open`

Imports: `src/ui/blocking_ops.rs:10`; `src/ui/branch_cleanup.rs:26`; `src/ui/branch_cleanup.rs:164`; `src/ui/branch_menu.rs:5`; `src/ui/commands.rs:39`; `src/ui/commit_list.rs:11`; `src/ui/commit_list.rs:590`; `src/ui/commit_list.rs:657`; `src/ui/commit_panel.rs:17`; `src/ui/conflict_binary_view.rs:27`; `src/ui/conflict_binary_view.rs:28`; `src/ui/conflict_editor.rs:49`; `src/ui/conflict_editor.rs:1268`; `src/ui/conflict_view.rs:40`; `src/ui/conflict_view.rs:453`; `src/ui/conflict_view.rs:1678`; `src/ui/context_menu.rs:5`; `src/ui/detail_panel.rs:12`; `src/ui/detail_panel.rs:248`; `src/ui/detail_panel.rs:282`; `src/ui/detail_panel.rs:313`; `src/ui/diff_cache.rs:7`; `src/ui/diff_split.rs:14`; `src/ui/diff_split.rs:515`; `src/ui/diff_view.rs:14`; `src/ui/diff_view.rs:1337`; `src/ui/diff_view.rs:1459`; `src/ui/diffstat_bar.rs:29`; `src/ui/file_history.rs:19`; `src/ui/graph_solo.rs:13`; `src/ui/graph_solo.rs:166`; `src/ui/graph_squash.rs:26`; `src/ui/graph_squash.rs:27`; `src/ui/graph_wip.rs:28`; `src/ui/inspector.rs:25`; `src/ui/mod.rs:122`; `src/ui/mod.rs:437`; `src/ui/mod.rs:516`; `src/ui/mod.rs:2083`; `src/ui/mod.rs:2084`; `src/ui/mod.rs:2569`; `src/ui/mod.rs:2584`; `src/ui/mod.rs:3952`; `src/ui/modal_renderers.rs:27`; `src/ui/modal_renderers_commit.rs:20`; `src/ui/modal_renderers_input.rs:17`; `src/ui/modal_renderers_plan.rs:17`; `src/ui/modals.rs:7`; `src/ui/oplog_panel.rs:18`; `src/ui/oplog_panel.rs:187`; `src/ui/oplog_render.rs:34`; `src/ui/pr_conflicts.rs:20`; `src/ui/pr_merge_status.rs:18`; `src/ui/pr_mode.rs:29`; `src/ui/reload.rs:12`; `src/ui/sidebar.rs:18`; `src/ui/smart_commit.rs:22`; `src/ui/tab_view.rs:11`; `src/ui/tabs.rs:72`

## Final recovery inventory delta

The same AST collector was rerun after removing every temporary source hook.
The11-package DAG,113 fields,64 inherent impls across51 files,10 uniform lists,
2 variable lists and5 ListState constructors are unchanged. cx.notify443→442;
qualified-Git-prefix expressions/imports176/59→177/60. The extra dependency
supports mutation-side failure recording, not UI git2 access. The complete
baseline locations above remain baseline locations, not silently relabeled
post-edit line numbers.

All50 files in the baseline >800-line inventory remain in that inventory;
no new file crosses800. Source-only membership remains41. Only these lengths
change (other listed files retain their lengths):

| File | Before LOC | Final LOC |
|---|---:|---:|
| `src/ui/pr_mode.rs` | 2349 | 2343 |
| `crates/kagi-git/src/backend.rs` | 2041 | 2020 |
| `src/ui/branch_cleanup.rs` | 1064 | 1044 |
| `tests/gui_e2e_runner.rs` | 927 | 992 |
| `crates/kagi-git/src/ops/stash.rs` | 958 | 949 |

All11 Cargo manifests, Cargo.lock and ci/loc-baseline.txt were compared against
the starting revision and are byte-identical. No crate, dependency or LOC
ceiling was added. Native runner growth is fixture scenario routing and
platform teardown, not production code relocated to evade the ratchet.
