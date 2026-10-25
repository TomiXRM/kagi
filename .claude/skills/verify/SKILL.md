---
name: kagi-verify
description: Verify Kagi changes against fixture repositories, the native GUI, and the browser story harness. Use for runtime, fixture, or E2E validation work in this repository. Driving the real GUI does NOT require taking the pointer, and does not take the foreground either — except a scenario that clicks the tab strip, which needs a key window, so ask the user first or use a machine nobody is working on. Tier B uses scripts/pidclick.swift (CGEventPostToPid), which shows where it acts with the scripts/pidcursor.swift agent cursor. cliclick is banned for new validation.
---

# Kagi verification recipe

Canonical source: `.claude/skills/verify/SKILL.md`. Codex reads it through
`.agents/skills/kagi-verify`.
PRs changing verification seams, flags, scripts, or runner features must update
this skill and state `skill 更新済み` in the PR body (see Maintenance).

Choose the smallest evidence lane that proves the change. Tier A is deterministic
native UI state coverage, Tier B is a real running application, and Tier C creates
the Git states needed to exercise safety-sensitive flows.

**Read this before driving the GUI.** Two rules are load-bearing and easy to miss by
skimming, so they are stated here as well as where they apply:

- **Never use `cliclick` for new validation.** It moves the user's pointer and targets
  the frontmost application, so it takes the machine away from the user and aims at
  whatever happens to be in front. Tier B's `scripts/pidclick.swift` posts events
  straight to the process with `CGEventPostToPid`: the pointer does not move, the
  foreground application does not change, and the target is the window you named. The
  user can keep working while a scenario runs. The one exception is a scenario that
  clicks the **tab strip**, which needs a key window and therefore the foreground —
  see Tier B's launch pitfalls. Ask the user before running one, or run it on a
  machine nobody is working on.
- **Never run the full `gui_e2e_runner`.** Always scope it with `KAGI_GUI_E2E_ONLY`.
  An unfiltered run once opened roughly 1,400 windows and crashed macOS.

| Need | Lane |
| --- | --- |
| Deterministic assertions on real UI state, no windows on the user's screen | Tier A runner (`KAGI_GUI_E2E_ONLY` always) |
| A real running app, real clicks, without taking the pointer or foreground | Tier B `pidclick` (tab-strip scenarios take the foreground — see its pitfalls) |
| Git states for safety-sensitive flows | Tier C fixtures |

Published mise recipe (only after v0.45.0 is published; run on each named host):
- macOS arm64: `mise install github:TomiXRM/kagi@0.45.0 && mise exec github:TomiXRM/kagi@0.45.0 -- kagi --version && codesign --verify --strict "$(mise where github:TomiXRM/kagi@0.45.0)/Kagi.app"`
- Linux x86_64 and aarch64 (run separately on each): `mise install github:TomiXRM/kagi@0.45.0 && mise exec github:TomiXRM/kagi@0.45.0 -- kagi --version`

The macOS release tar must be named `kagi-<version>-arm64-macos.tar.gz`, never
`aarch64-macos`: Linux aarch64 updaters through v0.43 could match the latter
as a legacy Linux tar. v0.44.0 published both the platform-qualified Linux tar
and its transitional alias. From v0.45.0, require
`kagi-<version>-<arch>-linux.tar.gz` and no platform-less alias. The workflow no
longer opts into `--legacy-linux-name`. Inspect all 13 release assets and verify
checksum coverage as described in `docs/RELEASING.md`.

## Tier A — native GUI E2E runner

`tests/gui_e2e_runner.rs` is an opt-in macOS main-thread runner. It needs both the
`gui-e2e` feature and `KAGI_GUI_E2E=1`; use an exclusive target directory:

`KAGI_GUI_E2E_ONLY` is not optional — every invocation names the scenarios it
wants. There is deliberately no unfiltered example here to copy: without the
filter the runner opens a window per scenario, which is the ~1,400-window path
that crashed macOS.

For #1731 use `KAGI_GUI_E2E_ONLY='conflict_draft_persist_failure,conflict_abort'`
with KEEP_GOING. The draft failure scenario makes the real conflicts directory
unwritable, edits the native Result, and checks the durable Failed receipt,
bounded Error toast even in the active Graph workspace, and EN/JA host-close/quit
and Cmd+W last-tab-close sheets. Its active-window seam prevents ordinary short-operation toast suppression
from hiding mandatory autosave errors. Renderer-fed guard labels
are observed through `record_conflict_draft_guard`; this is not OS accessibility
evidence. Retry must retain the sheet while writes fail, then save exact live
text after permissions recover; discard must require a second explicit action.
Abort is refused at both plan opening and confirmation while the live failure
flag remains set, even if permissions recover and an older on-disk draft can be
saved again. These cases compare every repository file byte, including `.git`,
and retain the newer live text. The real rebase Skip failure also checks that
the same operation-neutral typed blocker reaches the EN/JA footer and toast.
Tier B repeats DrillLeaveResume §1c and DrillSequencers §1 on private fixture
copies with chmod 000, comparing the plain binary before/after and capturing
the close sheet and refused abort without changing the repository.

For #1760 include `conflict_draft_flush_boundaries` in that selection. It sends
the production Result Change event without draining the writer first, then closes
the host/last repository tab and checks the exact durable body before destruction.
Hold the real draft lock while painting the pending sheet: EN/JA must show
Saving conflict decisions, not failure/Retry guidance, before release lets close
complete. The failure matrix also retains a dirty ordinary editor alongside the
failed conflict pane: its scoped file count remains visible through both Discard
steps; failure Retry flushes only the conflict draft and leaves the dirty ordinary
file and sheet open until a separate explicit Save All. Discard leaves disk unchanged.
Its second EN/JA case edits again while Continue awaits a barrier and requires
the stale generation to leave Git byte-identical without opening an approval.
Ordinary input only queues; no test may infer durability from queue acceptance.
The same-turn Continue → tab-close case must await the real concurrent barrier.
`draft_autosave_settled_for_e2e()` observes the pane's latest completion watcher,
pending failure presentation and outstanding barriers; `busy_dismiss::wait_ui`
waits on this state rather than sleeping for 250 ms. Locked restore/handoff
fixtures baseline already-presented autosave receipts/toast identities before
checking the one additional operation-owned receipt; they never hide real errors.
Tier B types in `long-policy.txt`, checks failed flush → Retry/two-step Discard
and successful unflushed close, and compares synchronous/queued timings with the
ignored backend `autosave_latency_three_files_five_hunks_200kb` benchmark.
That benchmark measures capture/disk API latency, not end-to-end frame latency.
`conflict_draft_repaired_actions` restores real directory permissions and retries
Save/Abort without a close sheet; both must pass the current-generation barrier.
`conflict_draft_shutdown` queues edits in two live conflict panes, invokes the
actual GPUI shutdown hook without first draining autosave, and checks both durable
bodies. Non-cancellable OS termination has a shared 200 ms budget, not an unlimited
durability guarantee; storage errors/slow I/O retain an unacknowledged crash window.

For #1706 W3 use
`KAGI_GUI_E2E_ONLY='issue_conversation_enabled_input_copy,issue_conversation_offscreen_select_all,neighbor_plan_labels,rebase_plan_labels,plan_footer_long_branch,squash_evidence_,pr_fields_escape_focus,conflict_,pull_,queue_checkout_missing_ref_refuses,queue_checkout_linked_worktree_refuses'`
with `KAGI_GUI_E2E_KEEP_GOING=1`. Shared helpers own byte-exact repository
collection, presentation setup, squash observations and writer polling, never
scenario expected values. Presentation teardown restores language, zoom and
palette before saved settings drop; each matrix leg unmounts its own window.
Keep the runner's shared-state and window-leak checks enabled. Run
`cargo test -p kagi --test repository_state_test` with a fresh `KAGI_LOG_DIR`
for non-UTF8 witness bytes and absent/empty/error oplog regression coverage.
No product rendering or behavior changes, so Tier B is not required.

For #1635 / #1637 use `KAGI_GUI_E2E_ONLY='nagi_fixed_accent,graph_selection_wash,graph_selection_crossfade,graph_wip_hover_wash,system_accent_chrome,primary_button_states,settings_theme_select_ax,theme_custom,dark_elevation_palette,oplog_detail_select_copy'`
with KEEP_GOING. `nagi_fixed_accent` injects purple/red OS accents while
selecting both Nagi presets, checks native focus-ring and graph-selection
pixels, visibly blue active versus neutral inactive fills with rendered
CIEDE2000 ΔE ≥ 10, selected label contrast through graph fades, and retained
selected identity. The graph wash scenario also checks Nagi Dark WIP text/chips.
Pair this with the `nagi_` palette/contrast tests in `kagi-ui-core`: retain 4.5:1
ordinary text/code and 3:1 essential graph/focus strokes, at most four hue
families and perceptually distinct selection/diff fills, not a saturation cap.
Tier B compares Apple versus Nagi on the same graph/diff/change fixture in
Light/Dark, EN/JA, 100%/167% and wide/compact windows. Check three blue/green/amber
lane families, brighter Dark lane bands/ref chips, neutral remote/tag metadata
and ordinary syntax (only keywords/types blue), and green/red additions/deletions.
Compare bright blue active selection with neutral inactive selection, including
WIP/commit/stash labels and secondary text. Inspect neutral expanded Dark oplog
details. Contrast ratios alone do not establish visual distinction. Preserve
`A/M/D/R`, `+/-`, labels, icons, lane geometry and focus indicators; no operation
relies on colour alone. Inspect semantic action
buttons' native neutral/disabled paint at rest, hover and held press; release
outside to avoid a Git write.
Capture Settings/palette switching and saved-theme relaunch. Copy the plain
production binary to the evidence directory before building `gui-e2e`:
the feature build replaces `target/debug/kagi` with a harness-only shell.
Native screenshots and color simulation are not CUD certification.

For #1638 use `KAGI_GUI_E2E_ONLY='modal_count_contrast,modal_sections,modal_compact'`
with KEEP_GOING. `modal_count_contrast` opens the real local Commit Panel before
planning Discard All, so the dialog receives its three target paths and warning.
It measures native Metal count glyphs against the composed pill background in
Nagi and Apple Light/Dark at 0.7×, 1× and 1.67×, at rest, hovered, collapsed and
reopened; both target and section counts must remain at least 4.5:1. The count
probes exist only in `gui-e2e`. Counts and chip geometry survive disclosure,
and the fixture's HEAD, index/status and file contents remain unchanged.
Tier B compares the released and fixed dialogs in all four themes on the same
fixture and viewport, with normal motion enabled; also inspect collapse/reopen.
Cancel every plan. This checks neutral metadata, not semantic danger chips,
VoiceOver or platform-wide accessibility certification.

For #1633 use `KAGI_GUI_E2E_ONLY='modal_scale_,modal_compact,modal_sections,plan_recovery_commands,plan_recovery_noop,plan_recovery_ax,plan_equivalent_summary,unmerged_branch_delete_armed,dialog_a11y_roles,destructive_modal_return,modal_no_fallthrough,blocked_plan_command'`
with KEEP_GOING. `modal_scale_targets` uses a real 500-file Discard plan and
collects every row only when fully painted inside the list/clip, including the
last path. `modal_scale_notes` deliberately supplies 200 typed warning/blocker
notes as renderer stress input, not a claim about backend-generated counts.
Its 940×660 matrix covers Apple Light/Dark, EN/JA, 100%/167% and inert blocked
confirms. Ready ref-only Reset is inspected then cancelled without executing
the renderer-only stress payload (#1695).
`modal_scale_note_paths` traverses 200 unique structured paths and the adjacent
mandatory notes, both inside an independently bounded note panel and inside a
Merge card's shared body scroller, across the same theme/locale/zoom matrix.
The injected Merge preview is renderer stress data too, not backend merge output.
Footer bounds must remain stationary while either owner scrolls.
Use `measure_leaf` for scroll viewport probes: an absolute child follows content
offsets and can send subsequent wheel events outside the actual scroll region.
Retain target/required-warning visibility, default-closed supporting commands,
and separate note/body scroll ownership. Tier B must inspect real narrow
windows, reach the final file, exercise optional disclosure and compare both
Apple themes/locales/zooms; never execute a stress plan to validate presentation.

For #1695 use `KAGI_GUI_E2E_ONLY='destructive_modal_return,modal_scale_,plan_labels,neighbor_plan_labels,modal_no_fallthrough,blocked_plan_command,unmerged_branch_delete_armed,amend,discard,modal_compact,dialog_a11y_roles'`
with KEEP_GOING. `modal_no_fallthrough` also confirms a real ref-only Reset once
in EN/JA, checks retained index/working files and its single receipt, then arms
Force-with-lease without writing the remote and checks the exact target/lease.
`unmerged_branch_delete_armed` keeps the nonzero-loss two-stage regression and
adds a remote-retained, zero-loss branch deleted with one confirmation.
Amend/Discard scenarios must assert one approval for local recovery-backed
plans and retained arm for genuinely irreversible plans, not duplicate confirms.
Tier B compares release/fixed Reset, unpublished Amend, backed-up Discard,
zero-loss local branch deletion and Force-with-lease in EN/JA on the same
disposable fixtures. Cancel presentation plans; record both stages only where
the fixed operation still arms. Never represent an arm-only check as execution.
`modal_compact` and the `plan_recovery_` renderer cases explicitly install a
real backend Push plan through the existing modal setter. A normal safe Push
entry now runs directly (#1693); these fixtures must not perform that write
just to obtain a confirmation card. They prove presentation, not Push routing.
Backend regression filter `cargo test -p kagi-git review_1695_ -- --nocapture`
also covers recovery-only ref retention versus ordinary zero-loss refs, stale
branch reachability before execution, and Amend publication without upstream
or through another remote's descendant of HEAD. Publication/reachability
changes after approval must refuse before writes, not reuse a one-stage plan.
For safety fixes record assertion failures before implementation and successes
afterward; a compile error or a no-op filtered run is not red/green evidence.
For inline review follow-ups use `cargo test -p kagi-git --lib inline_1695_ -- --nocapture`
and `cargo test -p kagi-git --test amend_test inline_1695_amend_ -- --nocapture`.
These check the actual API plan projection, retention refs/annotated tags/symbolic
chains held until deletion, typed late-publication refusal, and external writes
blocked from real Git hooks while Amend runs. Assert external writes can resume
after guard release. The Amend guard freezes existing publication refs only;
new remote-ref creation is outside its namespace-lock guarantee (ADR-0023).


For #1646 use `KAGI_GUI_E2E_ONLY='sync_footer_cancel_visible,modal_compact,modal_scale_notes,modal_scale_targets,sync_to_remote_armed,modal_polish_editor_fs,modal_polish_remote,modal_polish_stash_after,primary_button_states,create_branch_warnings,branch_rename_neutral_state,stash_entry_identity,pull_fetch_preserves_conflict_blocker'`.
`sync_footer_cancel_visible` covers Apple Light/Dark × EN/JA × 100%/167%
at 1000×700 and 940×700. Both confirmation stages must keep positive,
nonoverlapping Cancel/Confirm bounds inside the footer, including expanded
recovery. It clicks Cancel, rather than using Escape to bypass an invisible
control, and checks repository/receipt state. Use transparent leaf probes for
these buttons: auto-width measurement wrappers change percentage sizing.
Tier B must show the full armed caption and clickable Cancel in the real
narrow window; compare the same language, theme, zoom and fixture before/after.

For #1630, retain a real divergent Sync fixture with staged, unstaged,
untracked and ignored files. Record Cancel and first-stage no-write snapshots,
the successful operation's persisted receipt and every backup object, then
restore using only its recorded `recovery_plan.commands`—no unrecorded
checkout-alignment command. Check HEAD, staged blobs/modes, all working-tree
bytes, config and remote refs. Separately exercise stale tracking refs and
sparse/submodule/filter refusals without a repository write or false success.
Capture EN/JA × Apple Light/Dark × 100%/167% narrow-window presentation.
Keep the issue open until its separate human screenshot review is recorded.

Menu behavior uses `branch_menu_compact,context_menu_keys_short,context_menu_keys_sidebar`.
Run the separate `inventory:57-CommitMenu` through `inventory:62-WorktreeMenu`,
plus `inventory:67-EditorTreeMenu`, `inventory:70-FileMenu` and
`inventory:71-InspectorFileMenu`, with `KAGI_INVENTORY_LANG=en,ja` and
`KAGI_INVENTORY_THEME=light,dark` for native captures. These inventory cases
include direct open methods, not proof of every real right-click gesture.
Changes/Inspector file cards are not the shared MenuGroup renderer: Changes
places its red Discard last without a separator; Inspector has no danger action.
Branch Delete stays in the top-level management group by ADR-0049 / #1253.

For #1699 use `KAGI_GUI_E2E_ONLY='branch_menu,branch_cleanup,conflict_,oplog_append_failure,run_success_unrecorded_requires_acknowledgment,fetch_failure_reaches_the_oplog,pull_current_'`
with KEEP_GOING. `branch_menu_merge_labels` compares real merged, squash-proven
and unmerged tips in EN/JA after the owner-bound classification; it temporarily
moves the unmerged tip's tree object aside to prove read failure keeps the final
menu label neutral, then restores the object. Pending/unknown labels stay neutral.
`branch_menu_classification_test` covers missing tip/ancestor commits and a missing
squash-proof tree at the Backend boundary.
`branch_menu_orphan_delete` also covers unrelated roots: a known-unmerged EN/JA
menu, a ready delete plan, arming without a write, and deletion only after the
second confirmation. Backend tests distinguish no common ancestor from missing
commit/tree objects without suppressing either read-failure regression.
`branch_cleanup_target_heading` checks the actual plan preview group and cancels
without deleting. `conflict_merge_stopped_toast` and
`conflict_rebase_stopped_toast` execute one/three-file conflicts and require a
localized non-success stopped notice, real unresolved index entries and disabled
Continue. The existing oplog failure scenario covers failed Fetch plus success
and Unknown attempts under a real sidecar lock.
`oplog_append_failure_conflict_stopped` locks the real oplog sidecar while merge
and rebase stop on one/three-file conflicts in EN/JA. Require the recording-failure
AppNotice as the sole result presentation, retaining stopped context and the
append diagnostic, one non-durable Partial row, no toast, no durable receipt/retry
and a released known-outcome lease.
`conflict_continue_stopped_result_owner` repeats a real rebase Continue stopping
at step 2 for EN/JA and current/departed owners, with healthy/failed append.
On failed append, assert the exact localized acknowledgment includes the
next-conflict stop and Partial classification, not Succeeded. This classification
uses the original execution outcome, before conversion into a Partial display
row. Clicking its rendered confirmation must not replay, skip or retry persistence.
`pull_current_up_to_date` covers equal/ahead strict/ordinary Pull without changing
local edits; its blocker neighbor rejects a conflict arriving during the freshness
fetch. Tier B compares
the released and fixed real-window menu, cleanup confirmation and conflict
settlement on the same EN/JA fixtures, preserving viewport/theme/zoom. Menu and
cleanup captures cancel; conflict operations run only in disposable repositories.


For #1628 use `KAGI_GUI_E2E_ONLY='inspector_directory_accordion'` with KEEP_GOING.
This includes wide, compact and frozen-clock directory motion scenarios.
Exercise nested collapse, compressed chains, equal basenames, Path/Tree retention,
Enter/Space/Left/Right, hidden keyboard/AX membership, unchanged leaf diff,
Generated independence, moving live hit targets and inert exit paint.
Keep neighboring `inspector_files_keyboard`, `inspector_generated_`,
`inspector_overflow_keyboard`, `inspector_more_motion`, `inspector_file_row_hit`
and compact Inspector scenarios. Tier B compares Finder list disclosure with
the real wide Inspector and compact Changes in Apple Light/Dark, using an
isolated repository with nested and long EN/JA paths. Capture normal-motion
intermediate and settled frames and an app-private Reduce Motion endpoint;
do not change the owner's OS preference. Stable directory probes use
`inspector-directory-<full-repository-relative-path>`; compressed chains retain
the full path, and source-index file/ghost probes remain unchanged.

For #1479-S4 use
`KAGI_GUI_E2E_ONLY='sidebar_,input_names_inventory,input_ime_escape_boundaries,settings_,commit_panel_body_focus,compact_'`
with KEEP_GOING and a 300-second scenario timeout. Activate the native adapter
with `native_ax::activate`, then refresh and draw before inspecting its tree.
Naming assertions inspect `nodes[gpui_focus].aria.role/label`, not wrapper Groups
or the fork's rendered-Input recorder. The sidebar Clear tests cover localized
AppKit Press and pointer activation, empty absence and retained field focus.
IME coverage calls the public marked-text handler in Sidebar, Commit body,
Issue body and independent Settings, then dispatches Escape. This is native
action-routing evidence, not physical macOS input-method or VoiceOver evidence.
The old pin's Input action consumes IME Escape, but Kagi's negation-only global
binding intercepted Commit/Issue body keys first. Preserve the regression's
physical simulated Escape, rather than replacing it with direct action dispatch.
The same scenario opens the real toolbar PopupMenu and checks one Escape closes
only that menu (#1554), plus button-focused editing-action isolation. Both
TextInput and MultilineTextInput are real AX input roles; unrelated root focus
stops need not have a semantic AX node.
The `compact_` gate includes `compact_layout_state` and
`compact_layout_state_tall`: Escape in the co-author ListBox must retire its
owner and restore `cp-coauthor`, not dismiss the containing composer.
`compact_rail_nested_escape` and `compact_group_reveal_escape` check child-menu/
reveal dismissal before the navigator, preserving opener focus. Since #1640,
`compact_pr_menu_escape` instead verifies the explicit navigator-to-root-menu
handoff: the navigator closes before the context menu, Escape leaves the PR
workspace in place, and keyboard activation can reopen the PR rail afterward.
Do not expect focus to return to a PR row in the retired popup. The complete
Popover ancestry audit is in ADR-0014's #1479-S4 amendment.
For #1479-S1 use `KAGI_GUI_E2E_ONLY='terminal_,status_bar_,relative_time_ja,cleanup_local_date,oplog_row_typography,issues_row_secondary_text,finder_scale_rest,home_heading,inspector_scale'`
with KEEP_GOING. `terminal_font_fallback` checks real terminal configuration,
font/zoom updates, session changes and restart, with Regular/Bold Japanese face
references and terminal two-cell/cursor state. The native measurement probes
carry the same explicit fallback as production shaping.

Linux is an independent required lane, not a macOS substitute. Run
`cargo test -p kagi-ui-core linux_cosmic_text_uses_bundled_fallback_for_regular_and_bold -- --nocapture`
on Linux; it uses real CosmicTextSystem with system fonts disabled and compares
each Han glyph's selected face to explicit Noto Sans JP. For a real Nerd primary,
set `KAGI_TEST_NERD_FONT_REGULAR` and `KAGI_TEST_NERD_FONT_BOLD` to readable
JetBrainsMono Nerd Font Mono static files, then run
`cargo test -p kagi-ui-core linux_cosmic_text_nerd_primary_preserves_icons_and_japanese_fallback -- --ignored --nocapture`.
That explicit lane fails if the font files are absent. Retain the actual Linux
logs, architecture and tested commit; do not count the ignored default as PASS.
In an isolated container with Noto CJK SC/JP installed, run
`cargo test -p kagi-ui-core linux_cosmic_text_system_fonts_preserve_explicit_japanese_fallback -- --ignored --nocapture`
in separate `LANG=ja_JP.UTF-8` and `LANG=en_US.UTF-8` processes. This test
requires both competing families to be present and compares selected faces
against the same bundled JP reference; it is shaping evidence, not a capture.
The separate native acceptance in `docs/rearch/upstream-migration-1479.md`
requires fresh JA/EN processes, private minimal/system-Noto fontconfig,
X11/Wayland, terminal/reference images and the production diff/code, Inputs,
TextView, UI and status surfaces. Include the independently prepainted
`HoverHelp` tooltip, interactive `WorktreeHoverCard`, and `BranchDragGhost`:
they do not inherit Kit Root's font. `hoverable_tooltip` bypasses `HoverHelp`;
the explanatory-tooltip CI rule intentionally permits this interactive API.
Read their effective inherited font in a child prepaint probe and compare Han
FontIds with an explicit native Noto Sans JP reference; checking another freshly
constructed `ui_font()` would miss this regression. Capture detached overlays
separately (GPUI does not show them together). The system-Noto × JA negative
control must fail if the tested root's explicit
`.font(ui_font())` is removed. Never delete a user's fonts or change host
fontconfig. #1479-S1b removed the patch on registry cosmic-text 0.19.0, with a
review correction adding these independent-root cases; repeat the expanded
matrix for the later #1479-S6 GPUI cutover.
The detached-tooltip TestWindow regression runs with
`cargo test -p kagi-ui-core --features gui-e2e,gpui/test-support tooltip::tests`.
Retain native `toast_tooltip_lifetime`, `branch_drag_`, and `worktree_inspection`
regressions as well; drag and interactive-card cases read the actual inherited
font during prepaint. The worktree-card review correction observed a targeted
X11 × JA × system-Noto FAIL → PASS; it did not rerun that card across all eight
Linux combinations. Include it in the full matrix for the later GPUI cutover.

For isolated Xvfb/Weston software rendering, install `mesa-vulkan-drivers`,
`libgl1-mesa-dri`, `mesa-utils`, `vulkan-tools`, `x11-apps` and ImageMagick
inside the container only. Check `glxinfo -B` and `vulkaninfo --summary`.
Mesa lavapipe can initialize successfully yet present black X11 frames.
The current pinned GPUI does not consume `WGPU_BACKEND` when constructing its
backend mask: `WGPU_BACKEND=gl` alone is not sufficient. In that isolated
environment, `LIBGL_ALWAYS_SOFTWARE=1` and `VK_ICD_FILENAMES` naming a
nonexistent private ICD file force discovery to the working OpenGL fallback.
Capture X11 with `import -window <PID-resolved-window-id> <png>` or xwd.
For native Wayland, unset `DISPLAY`, set the private `WAYLAND_DISPLAY`, and
run Weston with a rendering backend (`--use-pixman`, not the headless
no-render default). A nested X11-backend Weston capture is compositor output,
not an Xwayland application capture; record the native app PID and isolate
the compositor to that app. Inspect actual glyph pixels and retain the
face-identity assertions; merely counting non-black pixels is not acceptance.

For #1514 use `KAGI_GUI_E2E_ONLY='update_,notice_'` with KEEP_GOING.
`update_notice_in_flight` queues an installer completion with
`e2e::queue_update_install` before clicking the real Update now button; it
disables startup checking before mounting, so that scenario never contacts the
update service. The held failure checks EN/JA × Apple Light/Dark × 100%/167%,
inert pointer/keyboard Skip, explicit continuing Hide, retained offer/status,
duplicate-start suppression and restored Skip/Later after failure. The seam
is compile-gated; it does not install or relaunch the working app. Plain builds
have no deterministic update fixture, so Tier B for this ticket is waived.

For #1498/#1499, `KAGI_GUI_E2E_ONLY='oplog_,issues_'` retains the existing
expansion/copy/keyboard oracles and adds `oplog_row_typography` and
`issues_row_secondary_text` (`tests/recovery/density_rest.rs`). Their paint-stage
font and vertical glyph-mask assertions cover EN/JA, Apple Light/Dark and
100%/167%. Tier B compares the live Operation Log and Issues navigator.

For #1640 use `KAGI_GUI_E2E_ONLY='compact_,settings_'` with KEEP_GOING and
`KAGI_GUI_E2E_TIMEOUT_SECS=300`. Compact membership is logical window width
below configured rem × current root rem, default 70rem (1008px at 100%).
`compact_rem_transitions` covers live preference/zoom/width changes and shared
draft/input/session state. `settings_compact_threshold_ax` exercises bounds,
persistence, native names, pointer activation and keyboard-first reveal in
enlarged JA Settings; manual wheel scrolling must not snap back to focused controls.
`smart_commit_settings_consent` reaches the real switch through Tab navigation
and requires it inside the host's scroll body before Space; model pointer paths
use wheel events instead of assigning the scroll offset. Native and portable
hosts retain their explicit-consent, persistence and no-generation assertions.
`pr_peek_visible_` retains table/sidebar, unloaded-head, dirty-editor and draft
coverage across wide and compact rem widths. A successful compact Peek must open
Changes and expose its real Compare file consumer, not merely publish hidden data.
`compact_layout_content_fit` covers short→long→short composer sizing, reopen,
viewport/zoom changes and retained input owners without a fixed empty footer.
Desktop-only stress fixtures either resolve an actual wide width or explicitly
isolate a supported 40rem threshold to preserve their original narrow geometry.
Tier B: compare actual logical dimensions, persisted settings and zoom on macOS
and Ubuntu X11/Wayland; distinguish output pixels from logical viewport size
and verify OS scale is not multiplied into the breakpoint again.

For #1225 compact layout integration, use
`KAGI_GUI_E2E_ONLY='compact_layout_,commit_panel_,home_,toolbar_overflow,bottom_panel,analyze_a11y_,pr_detail_a11y_'`
with KEEP_GOING and the unchanged 120s per-scenario deadline.
The geometry matrix is partitioned across `compact_layout_geometry` (660px,
100%), `_zoom167` (660px, 167%), `_tall` (1000px, 100%) and `_tall_zoom167`
(1000px, 167%). Each runs all EN/JA × Apple Light/Dark cells through the
same assertions and real rail interactions; the broad filter selects every
partition. This bounds process runtime without dropping matrix cells.

For #1491–#1494/#1502 use `KAGI_GUI_E2E_ONLY='compact_'` with KEEP_GOING.
The seven-trigger AX matrix is a heavy scenario: use the permitted 300s native
deadline rather than dropping EN/JA × 100%/167% × Apple Light/Dark cells.
`compact_rail_cmd_f` retires the harness-provisioned filter for its first-shortcut
leg, then checks the same rendered Input entity/value on reopen and at 1000px.
`compact_rail_short_window` uses real 940×400@167% / 400×300@100% windows,
wheel/pointer and Tab/Shift-Tab reveal plus the production legacy-size restore.
With Local retaining keyboard focus, wheel input must still reach Issues and
preserve the resulting offset across subsequent frames; focus reveal is one-shot.
`compact_popover_sections` checks shared PR disclosure and Issue category
callbacks, native focus-ring pixels and actual AccessKit role/name/state nodes.
`compact_rail_ax` reads those nodes after keyboard toggle, Escape, outside click
and owner/width/modal transitions. The native NSView accessibility entry point
activates the real adapter; this is not a claim of VoiceOver speech.
`compact_pr_popover_scroll` uses 33 natural-height PR cards, actual thumb drag to
the track end, policy hide/reveal pixels, filter/disclosure extents and a fresh
session-owned handle after repository switching. Observe inside flexible
containers: wrapping them in an auto-sized measurement div changes the layout.
The PR dashboard and Issue feed must not simultaneously mount the navigator's
shared filter Input/chip focus slots. Tier B: compare the provided plain baseline
with an isolated plain build at 900×500 in Apple Light/Dark, including high zoom,
scroll to Issues, and open Cmd-F; capture only the launched PID's window.

For #1652 use
`KAGI_GUI_E2E_ONLY='pr_section_pointer,pr_detail_,pr_lane_,compact_rail,compact_popover,compact_pr_popover_scroll,toolbar_pr_sidebar'`
with KEEP_GOING. `pr_section_pointer` covers horizontal Started inside followed
by Ended/Cancelled outside, including a sidebar hidden before the terminal;
subsequent gestures and Mine disclosure must remain usable. Its overlapping PR
sections retain distinct actual element/AccessKit identities. Pointer down and
up straddle a drawn frame; bounded real wheel input reveals the complete Mine
header before clicking. Record the scroll container with the layout-neutral
`measure_leaf("pr-sidebar-list-viewport", body)`, not a `measure_inside` child
that translates with its contents and makes a false viewport.
`pr_detail_adaptive` activates Graph in every layout, the leading chrome sidebar
toggle when noncompact, and the existing PR rail popup when compact.
Its activated native AccessKit nodes must expose the current Show/Hide action
name and toggled state, independently of the visible Graph caption. Do not use
the pinned Button's `.label()` for these controls: it overwrites the AX name,
even for an empty icon-only label.
`toolbar_pr_sidebar` sweeps 580–1000 base widths at 100%/167%, EN/JA and both
Apple themes, with both toolbar presentations and an offered update. It restores
the temporary 40-rem compact threshold and checks every visible control against
the toolbar viewport and its neighbors. Before the fix, the 596px / EN /
IconAndText leg overlapped repository identity x=86.5..248.5 with Update
x=230..326. Budget the opener's measured wrapper including its trailing padding;
that wrapper has no focus border to add to the width.
Tier B: reproduce outside termination in an isolated plain app, click Mine,
hide/reopen the sidebar from the dashboard, then open a PR and toggle Graph.
Compare the prior Navigator/Swimlane controls with the new placement in EN/JA,
Apple Light/Dark and normal/high zoom. Native AX metadata is not VoiceOver speech.
Do not equate direct `TouchPhase::Cancelled` coverage with native macOS phase
delivery: the pinned backend converts NSEventPhaseCancelled to Moved (#1654).
The real-window Ended recovery passes; native Cancelled recovery remains tracked
there. Inactive windows intentionally suppress hover help; use the existing
active-window native seam for tooltip construction, not a foreground takeover.

For #1513 use `KAGI_GUI_E2E_ONLY='palette_'` with KEEP_GOING.
`palette_rows_ax_action` shares the native NSView activation and AccessKit node
inspection helper with the rail. It checks result-owned Click actions and sends
AppKit Press through the production adapter, with exact dispatch identities and
counts for commands, themes and languages. Mouse down/up and Input Enter share
the oracle; disabled results dispatch nothing. This proves native adapter
delivery, not VoiceOver speech or IME behavior.

For #1417/#1418 use
`KAGI_GUI_E2E_ONLY='system_reduce_motion,system_reduce_transparency'` with KEEP_GOING.
The in-memory system-display injection never writes macOS preferences. Both
scenarios cover 940×660 / 1392×883, EN/JA and 100%/167%. Motion counts the actual
loading/spinner animation-construction branches and freezes the panel clock to
require immediate native pane geometry after system/app opt-ins. Transparency
checks the production tile token's alpha/restoration, label contrast and Metal
padding pixels of an actual PR tile. These injections do not prove native
NSWorkspace notification delivery. Tier B: toggle Reduce Motion/Transparency in
System Settings with an isolated running Kagi, compare pane/spinner and PR tile
before/after without restarting, and restore both OS preferences afterward.

For #1349/#1350 use `KAGI_GUI_E2E_ONLY='settings_'` with KEEP_GOING.
`settings_theme_select_ax` and `settings_zoom_stepper_ax` inspect renderer-fed
role/name/value/expanded/disabled properties on Appearance controls in the native
Settings host, across EN/JA and Apple Light/Dark. Theme semantics are assigned
to the kit's actual focused Select root; confirmation and Escape preserve its
identity and focus owner. Zoom retains kit activation and discrete caps, hands
focus to the opposite stepper at each bound, and Tabs onward to Compact graph.
These probes are not native VoiceOver or Full Keyboard Access evidence.
Tier B inspects the live ComboBox and option names and both zoom button/value
nodes at 100%, 70% and 167%, including disabled announcements.

For #1591 use `KAGI_GUI_E2E_ONLY='toast_,toolbar_overflow,settings_groups'`
with KEEP_GOING and `KAGI_GUI_E2E_TIMEOUT_SECS=300`.
`toast_tooltip_lifetime` measures the actual dismiss-tooltip content, requiring
a visible hover positive control before leave, re-render, live trigger movement
on resize, removal, preview replacement and timed exit; removal during the show
delay must not create an orphan. `toast_tooltip_scroll` sends a real wheel event
inside the 30%-height stack and collapses an older hovered card through its
rendered accessibility action. Both cover EN/JA and clear old measurements before
asserting absence. Lifetime first checks that the real inactive native window
shows no help without any override. Positive/lifecycle cases then use the
window-scoped `gui-e2e` active-input seam, not an alternate visibility branch.
Pair with
`cargo test -p kagi-ui-core --features gui-e2e,gpui/test-support tooltip::`:
TestWindow checks cover no-input movement, deactivation before/after the show
delay and fresh-hover recovery without that seam. Ordinary workspace tests
also cover the pure cancellation predicate. Run
`uv run --project ci check-all --selftest` and `check-hover-tooltip` to guard
registration. These are not macOS foreground-activation evidence. Tier B remains own-PID hover/leave and
deactivation screenshots in the isolated application; a locked screen cannot
supply that evidence. The shared boundary must not add children to Kit Buttons,
whose icon-only sizing depends on their child list.

For #1336/#1337/#1338/#1341 use
`KAGI_GUI_E2E_ONLY='diff_header_identity,toast_live_status,worktree_inspection_actions,clone_progress_status'`
with KEEP_GOING. Diff identity observes the invoked full-path Tooltip and
renderer-applied sanitized AX description while preserving header/hunk bounds.
Its fixture uses supported 160px sidebar/240px Inspector widths so the high-zoom
940px leg has a usable center slot; it does not claim to repair workspace
overcommit with two default-wide side panes.
Toast coverage checks four kinds and six consecutive arrivals per kind, stable
newest-arrival role/name/identity through hover/ticks/animation and no focus
stealing. The pinned GPUI API has no live-region/priority setter; these are
Status metadata assertions, not native speech or exactly-once announcements.
Clone checks operation-named elapsed Status across Reduce Motion and running
ticks, keyboard Hide owner retention, and real success/error settlement.
Inspection checks the wide pane's row Enter, Tab to named Refresh/local URL
Buttons, Enter/Space once-only activation, inner-focus retention, Escape row
return, owner departure and no-port absence. Compact viewports open the real
Worktrees rail and check Enter navigation/dismissal for both port states instead
of expecting the wide inspection card. Both layouts retain pointer-only hover
actions, painted extra-small URL chip text and native Tab-map exclusion.
Inspection covers 1000×720/1392×883, EN/JA, 100%/167% and Apple Light/Dark;
the neighboring status scenarios use 940×660/1392×883. Neighbor filter:
`KAGI_GUI_E2E_ONLY='diff_,toast_,clone_,worktree_inspection'`.
Native VoiceOver, Full Keyboard Access and real-window focus remain Tier B.

For #1310 use `KAGI_GUI_E2E_ONLY='status_bar_fit,status_,bottom_panel'`.
`status_bar_fit` mounts the production footer at 700×500 and 940×660 with
1/120-character branch identity, status chips, refresh timer and Busy/Failed
feedback. It checks full branch Tooltip/AX metadata, positive feedback width
and nonoverlapping in-window panel controls across EN/JA, 100%/167%, both
toolbar presentations and Apple themes. Probes are flow-neutral; renderer-fed
metadata is not native VoiceOver delivery. Real hover/Tab and before/after
screenshots remain Tier B.
Review legs `status_bar_feedback_min` and `status_bar_short_branch` use the same
matrix to preserve short Failed/Busy feedback beside a 25-character branch and
the natural `main`-to-dirty-chip gap, unellipsized rounded text width and right
footer padding with every chip (including at 700×500/167%). They fail before the
feedback-first/natural-width/rounding policies. The original fit scenario also
checks sanitized C1-control branch identity through the label/Tooltip/AX path.

For #1321, `status_bar_busy_height` uses the same matrix and requires Busy
feedback height not to exceed the actual footer height, with all feedback
bounds contained inside the footer. The pre-fix spinner/text block stack was
26.5px high inside a 20px footer at 100%. `status_bar_feedback_min` measures the
text child's actual bounds, not its spinner-inclusive container; Busy reserves
the unchanged scaled 10px spinner plus 4px gap in addition to its text budget,
capped by actual remaining room. Run `KAGI_GUI_E2E_ONLY='status_'` with KEEP_GOING.

`primary_button_states` (#1076) sends real hover / mouse-down events to a
modal Primary and compares native Metal fill pixels for every built-in theme.
The `gui-e2e` feature forwards `gpui_platform/test-support` as well as GPUI's:
without the platform feature `Window::render_to_image` returns the default
not-implemented error. Default application builds keep capture disabled.

For #1515 use `KAGI_GUI_E2E_ONLY='editor_history,editor_'` with KEEP_GOING.
`editor_history_fetch_error` queues an `Err` through `e2e::queue_editor_history`
at production `start_editor_history_load`; it does not seed the view directly.
EN/JA × Apple Light/Dark × 100%/167% checks cause/empty separation, real Tab
and Enter/Space Retry dispatch, successful commits, successful zero entries,
stale A-after-B rejection and mode/commit ownership. The cause includes ESC/BEL
and 500 long lines: raw state remains complete, renderer-fed text contains visible
escapes and a bounded preview, and native Retry bounds remain inside the History
viewport throughout the matrix. The seam replaces only the transport Task and is
compile-time GUI-only. Tier B uses a disposable
repository copy and a real Git read failure, restores it, then retries in the
same pane; screenshots and native AX are separate from renderer-fed text.

For #1674 use
`KAGI_GUI_E2E_ONLY='editor_markdown_default_view,editor_snapshot_readonly,blame_toggle_a11y'`.
The Markdown scenario traverses to the actual named mode Button, sends
Shift-Tab/Tab and repeated Enter/Space, and delivers native Press in both
directions while preserving the dirty Input entity and draft bytes. Existing
Undo/Redo, Save and historical snapshot checks remain in place.
Tier B: compare the same Markdown file in EN/Dark/100% and JA/Light/167% at
940×660. Use real Tab/Shift-Tab, Enter/Space, pointer and AXPress; confirm that
the action label changes without losing keyboard focus and inspect hover.
Read the settled native AX tree, not the initially unpublished lazy tree:
require one AXButton named Preview/Back to editor or プレビュー/編集に戻る
with AXPress. The adjacent Blame control is a useful positive control.
Record actual control bounds and screenshots; AX metadata is not VoiceOver speech.

For ADR-0226 use
`KAGI_GUI_E2E_ONLY='editor_markdown_default_view,editor_pdf,commit_pdf_,pdf_media_,pdf_focus_retirement,graph_copy'`
with KEEP_GOING, plus the neighboring `editor_,settings_` scenarios.
Markdown coverage must open a fresh buffer for each default preference and keep
existing manual modes and dirty text intact. Writer scenarios that edit Markdown
must explicitly select Source: a hidden Input is not the default Preview surface.
The default-view scenario also opens Preview with both Git Undo and Redo
available, focuses the workspace root, and sends Cmd+Z / Cmd+Shift+Z: no Git
modal or history-cursor move is allowed. Repeat with a dirty Source buffer
switched to Preview, then require Cmd+S to persist the exact draft bytes.
For #1668, the same scenario compares native Metal pixels in the actual center
pane: dirty Preview differs from the saved document and matches the explicitly
saved draft. Tab round trips preserve that rendering; Source Undo/Redo retains
the draft history, and neither round trip writes the file. Tier B additionally
edits a heading, link, repo-relative image and Mermaid block without saving;
verify that all reflect the draft while on-disk bytes and Git state stay intact.
Mermaid needs both `mmdc` and its browser runtime. For an isolated proof, install
the matching `chrome-headless-shell` with the existing Puppeteer CLI under a
temporary `PUPPETEER_CACHE_DIR` and pass that variable only to the owned app.

For #1670 pair `cargo test -p kagi-domain text_safety` with the scoped
`editor_markdown_default_view` native scenario. Domain coverage keeps the
leading cause across LF/CRLF, trailing blanks and stack frames, distinguishes
missing details and escapes display controls.
Tier B starts with a unique, uncached Mermaid source and an empty dedicated
`PUPPETEER_CACHE_DIR`: the real CLI's missing-browser cause must remain visible.
Check EN/Apple Dark/100% and JA/Apple Light/167% at 940×660, scrolling the
Preview to its final wrapped line without painting over neighboring panes.
An owned PATH directory containing `mmdc` symlinked to `/usr/bin/false` exercises
the empty-stderr fallback only; it is not a substitute for successful rendering.
For genuine CLI absence, use an empty private `/.local/bin` PATH prefix plus
system directories: a bare system PATH triggers startup login-shell restoration.
Finally install the real matching browser into the dedicated cache and relaunch
with the original PATH; require the actual diagram for the same source.
Preserve source bytes, HEAD and staged path/OID/mode, then remove probe tools and
the temporary browser cache after the owned apps exit.

For #1672 pair `cargo test -p kagi-domain mermaid::` with the scoped
`editor_markdown_default_view` native scenario. Pure coverage separates 1x/2x
and fractional backing density from logical zoom and cache identity.
Tier B uses the real `mmdc`/browser pair above at 100%, enlarged UI zoom and
940×660. Record the owned window's logical bounds and physical screenshot size;
do not claim unobserved display hardware. Compare the generated PNG's visible
pixel bounds against the same diagram-only screenshot ROI, compositing
transparency onto the actual pane background. A fitting diagram must not
magnify the source raster merely because the display is Retina.
An owned PATH wrapper may log start/end/arguments while invoking the real
renderer unchanged: open several uncached diagrams, send a burst of Zoom In,
and require nonoverlapping conversions for the latest density rather than
every intermediate zoom. Return to a cached density while another conversion
is running; its late completion must not change the current diagram's pixels.
Scroll through every diagram and the final paragraph, verify an unsaved draft
and a real renderer failure, and preserve source bytes/HEAD/staged content.
Remove the owned wrapper, resize probe and temporary browser cache after exit.

Pair PDF native coverage with `cargo test -p kagi-ui-editor`: exercise real
parser/raster output, stale completion admission, bounded reads, page replacement,
rotation and backing-scale-independent logical zoom. The Japanese fallback oracle
compares individual glyph masks against font-free Noto outlines; dark pixels or
successful ToUnicode extraction alone do not establish Japanese rendering.
Committed embedded-font fixtures use OFL Noto only; encrypted geometry has no font.
The native PDF scenario also mounts a 940×660 Japanese window at 167% UI zoom:
with the bottom panel closed, its real scroll viewport must retain at least
200 logical pixels of document height. Icon controls retain localized AX names
and hover help, including the visually abbreviated `100%` reset action.
`editor_pdf_fit_width` and `commit_pdf_fit_width` exercise the real named Fit
Width Button through pointer, Tab/Shift-Tab, Enter/Space and accessibility, in
EN/JA including the compact 167% window. Use mixed-size PDF pages wider than the
manual 25% floor, compare real raster pixels and measured content/viewport bounds,
and require no horizontal clipping after Fit. Resize, disclose Details and replace
the source: numeric zoom must remain unchanged until another explicit Fit.
Previous/next must retain a sub-25% scale; Zoom In and 100% reset stay usable.
After fitting mixed widths, resetting must leave the current narrower page on
screen rather than anchoring the empty canvas beside it.
Historical old/new sides retain independent zoom, scroll and immutable sources.
Draw ordinary dirty views without forcing `window.refresh()` while awaiting PDF
updates; otherwise the harness masks the header/Details displacement regression.
Tier B compares the same generated mixed-page fixture before/after in English
and Japanese, including both historical sides, and preserves HEAD/staged content.
For #1691, add `commit_pdf_sync_scrolling,commit_pdf_sync_lifecycle` to the
scoped native selection. Exercise the real mode Button with pointer, Tab,
Enter/Space and EN/JA accessibility names; wheel, page keys/buttons and scrollbar
dragging must synchronize page-relative positions without changing zoom or
focus. A wheel in the unfocused pane supersedes older focus history. Disabling
sync preserves both positions; missing pages clamp without feedback, and
replaced comparison entities retire their old subscriptions.
`commit_pdf_continuous_sides` additionally verifies cached Home/End revisits
reuse the same images without new raster admissions. Component tests cover
byte-bounded LRU eviction, preview/full ordering and obsolete queued demand.
PDF wait loops must advance the executor clock for the 120 ms refinement delay
while still allowing real background workers to complete.
Tier B compares independent and synchronized reading on generated unequal-size
documents, then rapid forward/backward scrolling on a private real PDF.
Do not publish private PDF screenshots or equate CPU raster timings with
end-to-end scrolling latency.


`editor_pdf_readonly` also exercises continuous first/middle/last-page reading,
more than six simultaneously visible short pages, and a cached hidden viewer
that must not keep scheduling raster work. `EditorWorkspaceView::pdf_probe(cx)`
now reads the retained `PdfViewer` entity; `PdfViewer::pdf_probe()` observes each
historical side directly. Require actual rendered pages, not just cache entries
(which can contain errors), and enforce the adaptive `cache_entry_limit`,
64,000,000 decoded-byte cache bound and at most two global raster jobs.
`commit_pdf_sources`, `commit_pdf_owners` and `commit_pdf_continuous_sides`
exercise real Git old/new blobs versus different working-tree bytes, explicit
added/deleted states, renamed old paths, parse failures, signature-only PDFs,
source/session supersession and independent continuous scrolling. Final-page
pixel markers must differ from first-page markers. Activate the native AX adapter
before querying side labels. Keep neighboring `binary_diff_`, `diff_highlight`,
`commit_diff_off_thread` and `editor_details` coverage.
`pdf_media_sources` distinguishes commit, HEAD, index and worktree PDF bytes
using real page geometry and blue raster pixels. `pdf_media_bounds` checks
exactly 10 MiB versus one byte over on both historical sides, plus an oversized
signature-only working-tree PDF. Read failures must not become absent sides.
`pdf_media_symlink` opens an untracked symlink to a real PDF outside the fixture:
the new side must show a read failure, never its target, while the old side is
absent. The fixture and target bytes remain unchanged.
Pair with `cargo test -p kagi-git --test file_reads_test --test staging_test` and
`cargo test -p kagi-git --lib backend::file_reads::tests` for bounded prefix
reads, sparse files, growth during reading, and index-error fallback refusal.
Backend regressions cover internal/external/dangling symlinks and conflicted
unstaged bases: ours, ancestor fallback, absence without a HEAD substitute, and
exact byte-path matching even when distinct index paths are not valid UTF-8.
Do not mistake those backend byte-read checks for non-UTF-8 per-file diff support:
the existing pathspec guard refuses such diffs before media preparation. Retention
uses the immutable commit and file index, not its display title. In Tier B, switch
between supported PDFs with different page counts/content, then reopen the same
file and require its zoom/page to survive; preserve HEAD, staged content and bytes.
`commit_pdf_staged_rename` opens a detected rename through the real Commit Panel:
HEAD old path / index new path / worktree new path contain 3 / 4 / 6 pages.
The staged pair must show 3 / 4; restoring the old path and reloading changes the
pair to absent / 4, then selecting the unstaged entry shows 4 / 6. Previewing
preserves HEAD, staged blob and worktree bytes. Tier B compares a staged rename
before/after in the ordinary app and verifies that the old side is not labeled
as an added file and that the new side is not substituted with worktree content.
The absent-base case must also report `ChangeKind::Added` and no old path in
`unstaged_file_diff`, so PDF preparation preserves absence instead of showing a
read error. Smoke the real main-diff worker against a stage-three-only conflict
with a distinct HEAD PDF: old must be absent and new must use the working tree.
Tier B compares the same outside-target symlink before/after in the ordinary
app. For an actual add/add stash conflict, a backend smoke must identify the
stage-two blob despite an absent HEAD path and distinct working-tree resolution.
`pdf_focus_retirement` clicks both historical PDF bodies, then closes/replaces
or hides their owners and navigates Graph without another click.
`pdf_focus_retirement_preserves_input` keeps typing in a sidebar/modal Input
when the old document disappears. Tier B repeats body click → Escape → Down
in the ordinary app and compares the selected commit before/after the fix;
also inspect an oversized side beside a valid PDF and a neighboring text diff.
`hold_next_file_classification_for_e2e` holds the actual file-read result before
publication. Open an extensionless PDF with root/tree focus and Git undo history,
then send Cmd+Z / Cmd+Shift+Z / Cmd+S while the classification is pending:
no Git modal or save request may appear. Release the result and require the
real document's page count. Without the `EditorLoading` root context, Cmd+Z
opens Git Undo and this scenario fails.
Also open a real PDF over the 10 MiB read limit with both Git Undo and Redo
available. After loading settles to the size-limit placeholder, the same keys
must not open a Git modal or emit Save; HEAD and the document bytes stay intact.
This case has no parsed PDF object and must retain `EditorReadOnly` ownership.
The same scenario renames active and cached dirty text buffers to `.pdf` through
the production Rename confirmation, then sends Cmd+S and compares the actual
new-path bytes with the retained draft. A filename-only PDF presentation check
blocks that save; classify the retained buffer from its accepted content instead.
After saving, let the watcher reclassify the retained text as a PDF and use
Cmd+Shift+G without clicking another control. Graph must open: removing a focused
Input must not strand global keyboard shortcuts on its obsolete focus handle.
Hold a clean buffer's watcher read, edit its retained Input, and release the read:
the discarded result must clear its own loading flag and Cmd+S must save the
draft. Repeat with Save before the held read is released: the retained Input
must remain writable, and the late result must not overwrite the saved content.
The Home branch-copy case also dispatches the explicit menu action against the
private test clipboard, not only its graph shortcut; Home must leave it unchanged.
Tier B compares actual Markdown preference persistence, continuous PDF reading,
zoom/scroll/reset, narrow wrapped errors and Retina sharpness. In Graph, open a
modified PDF and independently scroll its left/previous and right/current sides
past the first page; compare distinct historical content with an intentionally
different working-tree file. Check added, deleted and renamed documents too.
For supplied real repositories, work in a private clone and keep personal PDF
contents out of public PR attachments; use generated documents for public proof.
Verify foreground PDF Save/Undo/Redo cannot mutate the repository, using staged
path/OID/mode rather than index stat-cache bytes. PDF previews remain read-only:
side-by-side documents have no computed difference highlights.
For #1665 compare PDF error/status text in Catppuccin Mocha and Apple/Nagi
Light/Dark, including EN/JA and narrow high-zoom wrapping. Use source fg/bg
contrast (at least 4.5:1 for normal text), not antialiased edge pixels; keep
disabled toolbar controls distinct from required explanations.
For #1666 also inspect the generic Editor placeholders with a valid UTF-8 file
of 50,001 lines below 10 MiB, an over-10-MiB file, a NUL-containing binary and
invalid UTF-8. Compare the five built-in themes and a custom `text_sub` override.
At 940×660 / JA /167%, the entire size explanation must wrap inside the center
pane rather than overlap the file tree. Selection, file bytes, HEAD and staged
path/OID/mode stay unchanged by inspection. Pair with the scoped native scenarios
`editor_pdf_readonly,editor_snapshot_readonly,editor_history_fetch_error,editor_toggle_keys`.

`graph_copy_selected_branch` checks distinct selected/current branches, no
selection, remote/tag-only refs, unborn repositories, detached HEAD with a local
branch, Unicode names and Input/Editor/Terminal/menu focus ownership. Preserve
the clipboard when the selected commit has no local branch; do not substitute
the current branch or the SHA. Keep the existing Cmd+C scenarios and their
configured hash fallback. Native tests use the private test clipboard;
actual system clipboard integration belongs only in an isolated VM without a
clipboard bridge, never the user's host clipboard.

`inactive_selection` (#1176) measures selected repository-tab and conflict-row native
Metal fills across key/non-key/reactivated presentation, with unchanged bounds,
selected session and selected file. `KagiApp::set_window_active_for_e2e` overrides only the
activation input under `gui-e2e`; it never activates an OS window or takes the
user's foreground. Pair it with `inactive_selection_tests` in kagi-ui-core
for all-preset primary-label contrast.

For #1468 use
`KAGI_GUI_E2E_ONLY='selection_on_accent,inactive_selection,sidebar_rows,pr_lane_selected,commit_panel_selection_hover,graph_'`
with KEEP_GOING. After #1485, `selection_on_accent` checks the native 30%
system-blue Graph wash over ordinary stripes, normal summary/ref-chip/secondary
label paint, non-key grey, retained identity/geometry and native primary-button
labels across all presets. Sidebar and Commit Panel selected segments must
instead paint neutral grey and retain ordinary foregrounds in both activation states.
See [ADR-0125 amendment](../../../docs/adr/0125-apple-hig-themes.md#amendment--2026-10-13-finder-style-on-accent-labels-1468).
Pair with UI-core `selection_on_accent` / `finder_accent` and system-accent
injection tests. The scenario also exercises selected PR sidebar cards and
enabled menu hover/keyboard highlights, including explicit child-label paint.
Tier B uses the plain build, isolated profile and a copy of the stress fixture:
Apple Light/Dark selected Graph/file/PR sidebar rows, neutral segments,
accent-filled primary buttons and menu-hover PNGs,
plus one owned Finder list window/file selection captured by window ID and
closed afterward. Active-window screenshots require owner-authorized brief
activation of only the launched PID, with the prior foreground app restored.
For #1485 run `KAGI_GUI_E2E_ONLY='selection_on_accent,graph_,inactive_selection,system_accent'`.
`graph_selection_wash` covers Apple Light blue / Dark yellow × 100%/167%,
both zebra stripes, lane-band tint beneath the wash, normal semantic chip and
label colours, ≥4.5:1 primary-ink contrast, retained geometry/selection and
inactive grey for commit/WIP rows. Updated pixel oracles: the Graph legs in
`selection_on_accent` and `system_accent_chrome`; the latter's left selection
bar remains opaque accent. Non-Graph #1468 fill/foreground oracles are unchanged.
Tier B compares selected Graph rows before/after in both themes and zooms.

For #1348 use `KAGI_GUI_E2E_ONLY='editor_find_controls_a11y,editor_'`
with KEEP_GOING. The regression opens the actual pinned code-editor SearchPanel,
compares its Button role/name/toggled assignments, measures real button bounds,
and hovers each control to observe invoked Tooltip builders in EN/JA,
940×660 / 1392×883, 100%/167% and Apple Light/Dark. `Window::focus_next`
traverses rendered stops; F19 records the control receiving the simulated key.
Enter/Space exercise navigation, case matching, replacement disclosure,
Replace/Replace all and Close. Escape returns to the owning code buffer,
replacement stays unsaved/undoable, and a live locale switch retains the query.
The ownership leg focuses the replacement-disclosure Button, then sends
Select All / Backspace / Delete / word deletion / Paste / Cut / Undo / Redo /
movement keys and asserts that the hidden code buffer never changes. It also
checks that the case Button cannot edit its containing query and that a native
padding click directs editing to Find. Actual Issue-body and Conflict-Result
hosts check initial and retained EN/JA names/help, without replacing their inputs.
The fork's `test-support` feature is enabled only by the Editor's `gui-e2e`
feature; normal builds retain no probes. This is renderer/tooltip/simulated-input
evidence, not physical Tab, Full Keyboard Access, VoiceOver or live IME.
Tier B: in the real EN/JA Editor window open Cmd+F, Tab through the controls,
inspect names/help/toggle states in Accessibility Inspector, activate them,
and check Escape focus return, composition, replacement undo and guarded Save.

For #1301 / #1302 use `KAGI_GUI_E2E_ONLY='activity_,bottom_panel'` with
KEEP_GOING. `activity_granularity_keys` checks the five existing periods'
Tab roles, localized names, selected state, one Tab stop, actual focus
handles, Left/Right/Home/End/Enter/Space dispatch and native selected-fill
pixels on hover in active/inactive windows. `activity_bucket_a11y` checks
0/1/24 buckets' Status names against UTC date/time + commits/merges/authors,
one roving stop, arrow/Home/End boundary inspection and identical immediate
hover/keyboard readouts. The actual chart/legend painter records successful
solid/dash/gap/knockout segment counts and the single-point fallback branch.
`activity_equal_series_encoding` compares native merge-gap pixels with nearby
unpainted pixels over equal commit/merge values and 0/0 baseline segments,
including the legend; do not derive the expected background from the painter's
theme token. `activity_single_bucket_encoding` requires a genuine two-dash merge
mark and knockout gap, not the former solid fallback or a clamped counter.
All four scenarios cover 940×660 / 1392×883, 100%/167%, EN/JA and
Apple Light/Dark; axes and legend must fit.
Pair with `cargo test -p kagi-domain activity` for per-bucket distinct raw
email aggregation, and `cargo test -p kagi-ui-core activity_bucket` for
localized 0/1/plural units. Renderer metadata is not native VoiceOver or
real Tab evidence. Tier B: open Activity, Tab into periods/chart, inspect
both ends with the keyboard, verify the selected fill on hover and the
dashed merge legend/line, then read each bucket in Accessibility Inspector.

For #1510 use `KAGI_GUI_E2E_ONLY='activity_'` with KEEP_GOING.
`activity_bucket_hit` and its Light-100/Dark-100/Dark-167 partitions deliver
pointer events to the actual painter-recorded coordinates of both series for
48/42/30/52 buckets. Each covers the ordinary 940×660 layout and exact
260/400/600 logical-pixel plots at its theme/zoom. The RAII `PlotWidth` probe
changes layout only: the production canvas and overlay receive identical bounds.
Bucket index, UTC Status date/count and immediate keyboard/pointer readouts
must agree; Home/Right inspection and End/Left/Right retain the matching mark
inside the focus region. The existing four activity scenarios retain empty,
single-point and coincident-stroke coverage. Use the heavy 300s native deadline
when running this matrix. Tier B: hover an actual chart mark at 167% in Apple
Light/Dark and capture the isolated app's own window ID; painter metadata is
not native VoiceOver evidence.

For #1311 use `KAGI_GUI_E2E_ONLY='conflict_'` with KEEP_GOING.
`conflict_text_keyboard` opens a real two-hunk / three-line merge fixture,
Tabs to the production per-pane action rows, roves across hunks without selecting them,
and compares keyboard Use/Both adoption with measured pointer selection. It retains
line-level Partial state, Result undo/redo, left-footer Prev/Next reveal and
responsive source viewports with 24px uniform rows at 940×660 / 1392×883,
100%/167%, EN/JA, Apple Light/Dark, with zero repository writes or receipts.
The F19 probe observes the actual focused owner; renderer AX metadata is not
native VoiceOver evidence. `conflict_resolver_rebuild` replaces a retained path's
hunk/line counts and activates rebuilt controls. `conflict_resolver_roles` checks
real merge/rebase side identities and distinguishes adoption from low-level order.

On `feature/conflict-hunk` (ADR-0227), keep the 57 conflict registrations.
`conflict_progressive_add_add` covers the four adoption choices, decided reopening,
Both as one undo checkpoint, exact partial add/add bytes, protected empty manual
Result, real Save and the fresh fully staged merge handoff. `conflict_reset_disarm`
uses the Result-header Reset all button and retains two-step confirmation,
file/hunk/blur disarming and writer-busy refusal through pointer and Enter/Space.
`conflict_result_segment` retains its registration name but now covers direct
Result input, repeated focus, hunk/file navigation and writer-busy refusal,
not removed Preview/Edit segments. `conflict_file_selection` checks the source
list, hidden count, single header action, saved navigation and no Git receipts.
Its native row-fill sample is window-local logical x=left+4, y=vertical centre,
converted once by `window.scale_factor()`: outside icons, text, badges and rounded
corners, not the old right/top sample. `conflict_continue_cache` still requires
explicit Reset before replacing a divergent manual Result.

Run the existing scoped selection with isolated logs:
```sh
KAGI_LOG_DIR="$(mktemp -d)" KAGI_GUI_E2E=1 \
  KAGI_GUI_E2E_ONLY='conflict_' KAGI_GUI_E2E_KEEP_GOING=1 \
  cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
```
Read existing `target/gui-e2e/<scenario>/panic.txt` and screenshots before a
rerun. Never repair a product focus failure by focusing a hidden test owner,
or relax exact bytes to make a failed line adoption pass.

Tier B compares the real window against H1–H7 from
`docs/ui/conflict-sidebar/build-hifi.js` and the FRD Visual design pass as superseded
by FRD A+C, using the same fixture, viewport, language, zoom and theme before/after.
Capture unresolved Use/Both rows, resolved state/Change rows, reopened, ready-unsaved,
locked Saved, hidden sidebar, dark and narrow/high-zoom states. Require borderless
source documents, aligned 24px action rows, a control-free 12px gutter band,
Result-header focused Reset and confirmed Reset all, total/resolved header counts,
left-footer navigation,
direct Result input/origin stripes, top-down unpinned sections, fill-only rows,
plain hidden count, one header Save/Next file action and retained toolbar icons.
Exercise pointer and keyboard choices, direct typing, IME, focus return and AX
in the live window; probes and screenshots alone do not prove those behaviors.
Inspect accepted Saved evidence excluding unrelated staged files, stale reads
losing Saved claims and stash having no saved entries. Independently check actual
Save/Continue/Commit bytes. Navigation must preserve Git and emit no receipts.
Use goal-only independent trials as additional evidence, not proof of human
novice or cross-language usability.
For #1317 use `KAGI_GUI_E2E_ONLY='graph_'` with KEEP_GOING.
`graph_columns_fit` uses an isolated 40rem threshold to retain desktop
1000×660/167% and compact 940×660/167%, requiring a scaled 120px subject minimum, contained author/time,
native author middle-ellipsis styling and an actually painted graph lane in EN/JA.
It pins the before x positions at 1392×883/100%, preserves saved width keys
and preferred widths through resize/zoom, and leaves existing divider-drag,
subject-tooltip, copy, selection and worktree scenarios in the neighbor selection.
The pre-compact 940px reproduction had subject x=886.5 width=0, author ending at
1031 and time starting there; these historical positions are not compact pins.
`graph_columns_compressed_drag` delivers positive native drags to both capped
dividers at 1000px/167%, requires preferred widths/settings to stay unchanged, then widens and
pins the original x positions. `graph_columns_inspector_fit` covers 1440px/167%
with sidebar 240px and Inspector 360px in EN/JA: subject floor yields below 120px
while author/time remain contained. Before review: GRAPH preference fell from
176 to 47.900204 after a capped expansion; the Inspector leg retained the full
subject floor instead of yielding. Both review regressions pass after correction.
Tier B: compare the two real viewports with a long author in EN/JA, resize/zoom
back and forth, drag both column dividers, relaunch to check saved preferences,
and scroll a many-lane graph. Renderer probes do not prove native AX or screenshots.

For #1355 use `KAGI_GUI_E2E_ONLY='pane_budget_'` with KEEP_GOING.
The desktop fit and sidebar-anchor scenarios retain 1000px at 167% using an
isolated 40rem threshold after #1640; the 700px leg requires a contained rail and
no desktop side-pane clips, with preferred widths unchanged. Pure unit tests
retain the sub-minimum non-collapsed fit cases.

For #1550 use `KAGI_GUI_E2E_ONLY='window_,restore_'` with KEEP_GOING.
`window_min_size` checks restore clamping and reads the actual `NSWindow`
`contentMinSize` (940×660) with objc2. Its renderer resize leg must first
observe 1000×720, then change to the inspected native minimum; beginning at
the expected minimum would permit an undelivered request to pass.
The hidden runner cannot prove AXSize delivery: same-process AX requests
returned -25208 and an external runner child returned -25204, including an
offscreen-visible attempt. The legacy NSWindow accessibility setter was a
no-op. Do not treat any of those as successful runtime resize evidence.
Tier B must use external Swift `AXUIElementSetAttributeValue(AXSize)` against
the isolated plain app's PID, request 200×150, and confirm 940×660 with both
AX readback and the PID-owned CGWindow bounds. Capture EN/JA at 100%/167%,
the compact rail and toolbar overflow; attach `evidence.json` with images.

For #1316 use `KAGI_GUI_E2E_ONLY='toolbar_,workspace_mode_toolbar,toolbar_a11y'`
with KEEP_GOING. `toolbar_overflow_priority` shrinks and expands native chrome
across EN/JA × 100%/167% × Apple Light/Dark, preserves Git-first priority and
requires Pull/Push below the old selected-only 100px-floor fit threshold.
`toolbar_no_overlap` includes 700×500 with long repository/branch names in both
toolbar presentations, alongside the existing 940/1000/1392 widths. It checks
every visible control's outer focus-border bounds and the real Git-group bounds
against workspace controls. Repository-group recording uses `measure_inside`,
not a feature-only wrapper that changes flex sizing. Absolute natural-width
canvases exclude the fixed 2px border on each side: icon-only fitting must add
those 4px just as caption fitting does, and hidden natural probes carry the same
border. The 500px-tall leg is geometry-only; existing 660px legs retain overflow
activation, shortcut, unavailable-reason and PTY-focus coverage.
Before the tiny-window correction, the native 700×500 / EN / Icon Only / 100%
leg recorded Terminal x=483.5..512 intersecting Graph x=495.5..524. Tier B should
compare the real 700×500 JA toolbar with a long repository/branch identity at
100% and 167%, then open »; bounds probes are not screenshots or VoiceOver.

For #1742 step ① use `KAGI_GUI_E2E_ONLY='conflict_preview_'` for the focused
preparation checks, then the complete ADR-0228 image/PDF/sync/focus/`conflict_`
regression filter. `gui-e2e` forwards `kagi-git/gui-e2e` solely for the
thread-local Backend-open witness: `conflict_preview_no_render_io` counts zero
opens in ConflictView render from the first uncached frame, holds the real
worker result to inspect header-only loading, then checks retained-image redraw
with the repository poisoned. A warmed cache alone is not this proof.
`conflict_preview_owner_transitions` holds actual completions across selection,
revision and same-key retirement/re-admission; its feature-only detach bypasses
task cancellation so the production generation/owner checks must reject each
late result without replacing the shown pair. `conflict_preview_read_retry`
checks terminal failure after two attempts, no repaint retry loop, and recovery
on a fresh observation. Pair with `cargo test -p kagi-git --features gui-e2e
render_probe` and `cargo test -p kagi-git --test file_reads_test`. The probe and
scheduling seam compile out of ordinary binaries. Tier B compares the original
merge-complex image/gitlink rows plus identical valid-PNG variants on isolated
fixture copies; never mutate the source fixtures or infer native AX/IME evidence.

For #1283 use `KAGI_GUI_E2E_ONLY='conflict_,stash_'` with KEEP_GOING.
`conflict_file_selection` enters the persistent `conflict-tree` ListBox through
real Tab/Shift-Tab and its GUI-only F19 focus probe, rather than seeking a removed
overview Open button or assuming the tree owns the window's last Tab stop
(#1222 added status-bar stops). Arrows rove focus across pending and saved rows;
Enter/Space opens the focused path. After pointer navigation, the product itself
must preserve keyboard entry; the test does not refocus the root. Cover the first
raw file, ready unsaved drafts, saved read-only inspection, and sidebar hide/show
without selection loss. Inactive fill and selected AX option must agree.
The stash scenarios Tab to the rendered plan Confirm and send Enter down/up
for Pop/Drop; danger sheets have no default button after #1767: root Return
leaves the same sheet open and neither confirms nor cancels. Follow-up coverage
checks that the retained stash has no write lease before explicit approval,
confirmed Drop records exactly one successful receipt, and root Return leaves
the stash/repository unchanged without a drop receipt. Duplicate-OID suppression
and close/reopen owner lifetime remain covered. These are native Tier A oracles,
not a change to product safety or Tier B evidence.

For #1298 / #1299 use
`KAGI_GUI_E2E_ONLY='settings_provider_keys,settings_analyze_ignore_feedback'`
with KEEP_GOING; run `settings_` for the neighboring baseline. These two
scenarios explicitly select the production 715px macOS Settings host in
EN/JA, Apple Light/Dark and 100%/167%. Provider/model selection uses the
renderer's named selected Tab recorder and real Enter/Space down/up after
focus-handle setup, with 0/1/20 models, unavailable CLI refusal, persisted
values and no generation modal. Analyze clicks actual visible/scroll-reachable
Save/Reset controls against a directory at the target path and a mode-000
settings directory: errors retain the draft/dirty marker and original bytes;
removing the obstacle permits Save/Reset and quiet confirmation. Toast dismissal
uses the real named close control, not direct stack mutation; keyboard dismissal
also Tabs to the close control while both hosts render the shared stack, then
checks pane Left/Right and native Cmd+W after removal. Tier B still owns
provider/model Tab traversal, focus-visible rings, VoiceOver names,
selected/unavailable state and native toast appearance.
The persistent-error host-handle unit uses opt-in GPUI test support, without
launching the native runner:
`cargo test -p kagi --features gui-e2e --lib closed_window_releases_persistent_error_close_handles`.

For #1732 use `KAGI_GUI_E2E_ONLY='conflict_draft,conflict_open,conflict_launch,conflict_file_selection'`
with KEEP_GOING. Corrupt restore covers EN/JA notices, exactly one Failed receipt,
exclusive `.corrupt` preservation, editable index-backed content and resumed
autosave. `RepositoryState::with_appended_oplog` constructs the expected single
canonical receipt append; it still compares every Git field and complete log
byte-for-byte rather than dropping the log from the no-write witness.
`conflict_draft_restore_io_failure` covers first-open failures with no retained
pane: unreadable saved edits (including a
readable older `.prev`), an unopenable lock, bounded lock contention and exhausted
rescue names in EN/JA. A successful append preserves the exact cause in one Failed
receipt and presents only one bounded, path-free footer or notification, never a
modal; an intentionally blocked append instead requires acknowledgment retaining
both causes, with no ordinary preview. It keeps the editor fail-closed without
per-frame re-detection and verifies unchanged Git/draft bytes plus only the exact
asserted receipt append.
`conflict_draft_stale_restore_failure` delivers an older real lock-timeout restore
and older real successful detections after a newer rebase conflict and failed
autosave. The exact pane, Result input, live text, run-once/review flags and pending
revalidation gate must survive both rejected outcomes, without an obsolete notice.
`conflict_draft_stale_restore_async` holds a real async error immediately before
admission, accepts a newer read or detection of the same Git state, and checks
that read keys and detector generations independently reject it. The hold is
GUI-feature-only and releases even on unwinding; it never substitutes a result.
`conflict_draft_fresh_restore_failure` types a manual Result while autosave fails,
then makes the draft unreadable and invokes quiet, manual and external production
Reload in EN/JA without changing Git. The exact pane, editor entity and pending body remain;
the Failed receipt owns the full cause and the footer stays bounded. A real Save
click must leave every Git field and complete log unchanged because the checked
pane read remains refused. Repairing persistence and explicitly reloading settles
that gate without replacing the editor.
`conflict_draft_revision_unrelated_index` also fails autosave, makes the draft
unreadable, stages an unrelated CLI file, and reloads in EN/JA. The changed global
observation cannot discard the exact pane/input, pending body, selection or
undo/redo; successful repair adopts the new revision without losing that history.
`conflict_draft_continue_restore_admission` opens a real sequencer confirmation,
then runs real graph paging and fails same-observation restore. Paging keeps the
ReadKey and bypasses Reload's modal sweep, so only failed detection can invalidate
the approval. The modal must close with the existing bounded failure preview.
Reinstalling that genuine approval and clicking Confirm separately
tests the consumer gate: no lease, staging, sequencer advance or receipt append.
`conflict_draft_recovered_save_frozen_payload` combines #1732 restore admission
with #1743 staging: a real unreadable draft retains the CRLF Result and editor
behind the closed gate without a Git write. After repaired explicit detection,
the real Save must record Success, retain the index-side executable mode under
`core.fileMode=false`, clean the frozen blob to LF under `text=auto`, and preserve
the approved CRLF worktree bytes. This is native Tier A, not a Tier B screenshot.
`conflict_draft_stale_restore_success` holds an older real successful detection,
then applies a newer real failed restore with the same Git observation. ReadKey
and detector-generation supersession are exercised independently; neither stale
success may clear the failure, settle the gate, or admit Save/Continue. EN/JA cases
also perform a real old-request corruption rescue: its bounded warning must still
surface on stale completion, without changing the newer failed pane or receipt.
`conflict_draft_abort_restore_admission` applies the same real paging failure to
an existing Abort approval, then reinstalls the genuine approval and confirms:
producer dismissal and consumer admission must independently prevent Git work,
leases and duplicate receipts; repaired Abort remains usable in EN/JA.
`conflict_draft_cleared_handoff` fails real autosave, externally aborts or completes
the sequencer, and requires the exact Result/input/selection/history to remain
until an explicit repaired Reload durably preserves it for restart inspection.
Its failed receipt owns the full cause and the bounded EN/JA save preview tells
the user to keep the window open; no render-frame retry may append another receipt.
`conflict_draft_stale_handoff` withholds a real durable writer completion, makes a
newer edit fail, and proves the older ticket cannot remove that newer live owner.
The hold is GUI-only and never replaces a writer result. Backend assertions also
require `has_unflushed_drafts` after a synchronous save failure with no queued ticket.
`conflict_open_fresh`, `conflict_open_second_tab` and `conflict_launch` exercise
production native-picker completion/CLI startup and real asynchronous accepted
reads in EN/JA, without corrective Reload or synthetic detector results.
The first workspace must belong to the accepted revision and have a rendered
Result with checked-pane admission settled. Tier B launches a plain binary into
Home with restore disabled, delivers a real Finder Open Documents event to that
PID, then opens a distinct conflicted copy as its second tab. Capture each real
window without Reload and compare the missing-workspace before screenshots.
The backend suite also covers failed autosave → preservation clone → save,
both concurrent merge orders, torn-temp/backup recovery, lock timeout and
SIGKILL release. Tier B uses two plain-binary instances sharing one isolated
`KAGI_LOG_DIR`; restart must retain both independent same-file hunk decisions.
For recovery-retention review, run `cargo test -p kagi-git --lib
two_writer_recovery_survives_other_file_autosaves`: two real subprocesses collide
on one Result, then the earlier writer repeatedly autosaves another file. Both
commit orders and blocking/queued saves must retain the later primary and the
earlier recovery's exact bytes/identity through the owned inspection API, in both
the primary document and repeatedly rotated `.prev`. The formal
`two-writer-recovery` check must HOLD; pre-merge-dedup controls must stay red.

For #1198 use `KAGI_GUI_E2E_ONLY=settings_window_` with KEEP_GOING for the
six production macOS host scenarios (EN/JA, 100%/167%) plus host-isolation
cleanup. Owner-release coverage starts a real fetch ticker, closes its repository
root and asserts the entity is released and Settings closes while another
repository stays usable. Position coverage reopens three times and exercises
repository Cmd-Q with Settings still open, quit-time flush and root recreation
without a Settings-close save. Dock coverage uses the production typed-root
decision and saved-session restore with only Settings remaining.
Existing `settings_*`, `platform_menu_*` and
`filter_menu_then_settings` scenarios deliberately select the portable overlay
host; none of their assertions are removed. An RAII host guard restores the
selection even on panic; ordinary launches always use the native macOS host.
Pair native geometry coverage with `cargo test -p kagi-ui-core settings_window_`.
The existing isolated-child store harness checks 100 production frame writes
cost at most two successful commits, preserve the final "x,y,height" value, and
migrate/remove the obsolete position/height pair exactly once.
Tier B must list **both** CGWindow IDs for the PID it launched and capture the
Settings ID with `screencapture -l<id> <path>`; PID-first capture selects the
repository rather than Settings. Check Cmd-comma raise, Cmd-W independent
closure, native traffic lights, restored position/pane/height (also after app
quit while Settings remains open) and live preferences.
Escape is deliberately not a window-dismiss command; check editing-field/IME
behavior separately. Native hidden-window assertions do not prove OS activation.
For #1227 use `KAGI_GUI_E2E_ONLY='commit_panel_i18n,commit_panel_viewport,commit_row_layout'`.
`commit_panel_i18n` draws Apple Light/Dark at 1392×883/100% and 940×660/167%,
changes JA→EN→JA→EN on the same panel, compares renderer-fed captions plus AX
name/role assignments with Msg, checks selector selection state, measures containment
and ≥20 logical-pixel hit heights, clicks Tree, and preserves Template data verbatim.
The feature name Commit Panel and operation-button names intentionally stay English.
The AX recorder proves the renderer assignment, not native VoiceOver delivery.
Tier B remains a real-window comparison of Japanese headers, batch actions and
unchanged user Template text; `commit_panel_viewport` retains composer geometry.

For #1306/#1307 use
`KAGI_GUI_E2E_ONLY='graph_subject_identity,inspector_full_message'` and retain
`graph_,inspector_,detail_` as the neighboring filter. The subject scenario
compares complete sanitized 71/72/73/300-character and CJK/emoji identities,
drawn AX names, 29-reference-px row density, author/time geometry, and real
row-hover Tooltip content and viewport-contained bounds in EN/JA, Apple
Light/Dark, 100/167%, at 940×660 and 1392×883. The 300-character subject
must wrap rather than overflow the narrow viewport. The row-wide Tooltip
also works when existing narrow, high-zoom column widths leave the visible
subject no space; this does not claim to fix that pre-existing column layout.

The Inspector scenario keeps 1,999/2,000/2,001/10,000-character messages plus
an 85-paragraph sanitized CJK/emoji message. Its renderer-fed source/HTML,
complete co-author/trailer counts and full-message clipboard assertions run
over the same matrix. A real wheel reaches the last trailer inside the
bounded message frame while the changed-files frame stays fixed; warm draws
leave SHA-keyed derivation counters unchanged. Native VoiceOver and real
selectable-text keyboard behavior remain Tier B.

For an opt-in stress measurement, set
`KAGI_TEXT_CAPS_STRESS_REPO=<read-only 121-local-branch fixture>` and select
only `KAGI_GUI_E2E_ONLY=text_caps_warm_render`. The native runner mounts that
real repository, selects its top commit, settles 20 draws, then prints the
median of 31 batches of ten forced warm draws (milliseconds per frame).
Compare the same fixture, harness, build profile and presentation state;
alternate frozen before/after binaries three times each after gates finish
to mitigate concurrent-compilation noise. This is warm render evidence,
not application FPS or a cold-load benchmark.
Without `KAGI_TEXT_CAPS_STRESS_REPO`, the stress scenario prints
`[gui-e2e] SKIP text_caps_warm_render` and returns before opening a window;
broader substring filters such as `caps` or `warm` do not require that fixture.

For #1229–#1231 use
`KAGI_GUI_E2E_ONLY='commit_panel_segment_keys,commit_panel_footer_tab_order,commit_panel_coauthor_keys'`
with `KAGI_GUI_E2E_KEEP_GOING=1`. The regressions deliver simulated keys to
production focus handles: one roving List/Tree stop; body → Template → Generate
→ co-author → Amend → available Commit; inert unavailable actions; Template
pressed state; and twenty long-name/email co-author candidates with tail
scrolling, duplicate-safe trailer insertion, empty candidates and Escape focus
return. AX assertions compare both role and localized name assignments. Pair
with `commit_panel_,commit_row_layout,smart_` for the existing neighbors.
These renderer/simulated-input assertions do not prove Full Keyboard Access,
VoiceOver or IME behavior; Tier B compares those in the real EN/JA window.

For #1199/#1201 use `KAGI_GUI_E2E_ONLY=settings_quiet,settings_radio_dot`.
`settings_quiet` measures inherited heading colour/size/weight and Metal pixels
for the frame/titlebar/section hairlines at 100%/167%, EN/JA, both Apple themes
and injected blue/purple accents; adjacent pixels enforce literal 1px width.
`settings_radio_dot` renders the actual pinned Radio component and checks a
symmetric contrast-preserving primary-foreground centre, surrounding accent
and empty unchecked input fill in both themes at both zooms, with explicit
Theme accent and blue/purple System accent (Dark Theme requires a dark dot).
Public `e2e::measure_control` permits native component probes to reuse the app's
bounds recorder; default builds retain its pass-through. Shared non-E2E
`measure_inside` placeholders are absolute and zero-sized: the unit
`measure_inside_placeholder_is_out_of_flow` prevents flex/gap layout changes.
These renderer assertions are not real keyboard, VoiceOver or Tier B evidence.

For #1200 use `KAGI_GUI_E2E_ONLY=settings` for the complete Settings matrix.
`settings_groups` checks semantic card bounds, native base-fill pixels and page
wheel ownership at 1392×883, EN/JA, 100%/167%; ordinary controls fit when
reached and the unchanged oversized Analyze editor has separately reachable
edges. It measures Analyze's vertical padding/gaps and ordinary descriptions
of at most two lines at 100%. Real mouse-move events plus the Tooltip delay
exercise avatar/custom-theme/Analyze help builders in both languages.
`settings_view::take_settings_help_for_e2e` consumes only text actually passed
to an invoked Tooltip builder; each hover clears old observations first.
This is renderer/tooltip-construction evidence, not native VoiceOver delivery.

For #1271/#1272 use `KAGI_GUI_E2E_ONLY=palette_` with KEEP_GOING. The
`palette_highlight` scenario compares native row-fill pixels with the one AX
selected result, moves the pointer to a different real row, scrolls beneath the
resting pointer (including same-coordinate move replay), and presses Enter;
unavailable rows stay inert. `palette_shortcut_glyphs` reads renderer-fed
captions and measures non-overlapping, right-aligned trailing-column bounds
at 940×660 / 1392×883, 100% / 167%, EN / JA, Apple Light / Dark.
Only the exact KEEP_GOING child for this scenario installs its saved remaps/
unassignment **before** `init_app`, because `effective_keystroke` caches settings
during production startup; changing settings after keymap registration would
not test remapping. Other same-process scenarios retain their normal bindings.
The guard restores those isolated startup keys after the run. Native pointer
delivery, IME and VoiceOver remain Tier B, not claims from TestDispatcher.

```bash
# One scenario: the substring the scenario name contains.
KAGI_LOG_DIR="$(mktemp -d)" KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='bottom_panel' \
  CARGO_TARGET_DIR="$PWD/target" \
  cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture

# Several: comma-separated substrings, any of which may match.
KAGI_LOG_DIR="$(mktemp -d)" KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='bottom_panel,graph_copy' \
  CARGO_TARGET_DIR="$PWD/target" \
  cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture

# Issue #462 compact cards plus the disclosure baseline they must not break.
# 600/700/750/900 x EN/JA, one hidden window at a time, all unmounted.
KAGI_LOG_DIR="$(mktemp -d)" KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='modal_compact,modal_sections' \
  cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
```

For #1175/#1182 use `KAGI_GUI_E2E_ONLY='pr_dashboard_responsive,palette_viewport'`.
The dashboard measures Apple Light table/viewport bounds at two widths, EN/JA,
100/167% and Scrolling/Hover/Always policies: Scrolling overlays with zero lane;
non-auto-hide Hover/Always reserve 16 logical px and pin the table bar to Always.
The 940px dashboard retains its compact rail while Cmd-B changes the shared
Sidebar preference. At 1440px, the default 70rem threshold gives inline Sidebar
at 100% and compact rail at 167%; assertions follow the central rem predicate.
Neither viewport is substituted to satisfy the old inline-only oracle.
X/Y routing and trailing-column reachability stay intact. `e2e::chrome_border`
records explicit rendered border styles for table
rows/header and palette frame/divider; it is not a screenshot-pixel sample.
Pair with the existing `palette_selection_scroll`, `palette_push_modal_keys`
and toolbar scenarios. Real fading, macOS preference changes and JA IME are
Tier B comparisons, not TestDispatcher assertions.

For #1352 use `KAGI_GUI_E2E_ONLY='pr_dashboard_'` with KEEP_GOING and a
120-second child timeout. The draft-contrast matrix is split into four
Light/Dark × narrow/wide children so every native capture fits the timeout,
without dropping EN/JA, 100%/167% or rest/hover × active/inactive coverage.
Production cell bounds select actual Metal glyph-interior and blank-background
pixels for titles, number, author, age and the localized Draft state; inherited
row opacity is included in the captured pixel, not replaced with theme RGB.
Draft/ready rows retain identical title/author/age facts and the ListItem name.
The same children also draw a local draft with Conflicting status: a GUI-only
fixture seam seeds L2 freshness, while production classification must retain
Needs You's blocker-coloured STATE. The plain Draft reason alone is muted.
There is no selected dashboard row state. Pair with the ui-core
`apple_pr_dashboard_draft_text_meets_wcag_aa_on_row_backgrounds` unit test.
Tier B remains a real-window comparison of draft/ready rows and opening a draft
in Apple Light/Dark; native VoiceOver and Full Keyboard Access are not proven
by the recorder or image contrast.

For #1016 PR B use `KAGI_GUI_E2E_ONLY='modal_polish_'` with
`KAGI_GUI_E2E_KEEP_GOING=1` (`tests/recovery/modal_polish_b.rs`). The three
scenarios draw the Editor name form (including invalid names, valid action,
Delete `.git`, and Trust Repo), Remote Browse's idle/busy connect button, and
Stash Drop's localized AFTER chip with its full AX and Copy all text. Each
unmounts its own window; keep this filter rather than launching unfiltered.

It uses `VisualTestAppContext` with deterministic assertions. Its test windows
can appear at the primary display's top-left while a scenario runs; each must
disappear when the scenario ends, and none may remain when the runner exits.
Do not use it to wait on a child process or to validate IME/focus behavior:
`TestDispatcher`'s `run_until_parked` waits while a job is outstanding. Each
new scenario must call the runner's shared `unmount(cx, entity, window)` helper,
which calls `remove_window`, drops the root entity, flushes the app, and then
drains the dispatcher. Drop retained child entities before that helper when
needed, otherwise the runner's leak detector can retain them.

`KAGI_GUI_E2E_ONLY` is a comma-separated, case-sensitive substring filter.
Whitespace is trimmed and empty entries are ignored; only an unset value runs
the full suite. An all-empty value or a filter with no enabled scenario match
exits nonzero after reporting each scenario as filtered out.
**Do not run the full suite by hand.** A full run once opened 1,408 real
`NSWindow`s at once and the WindowServer watchdog killed the login session
(#549); always scope it with `KAGI_GUI_E2E_ONLY=<substr>`. Windows now come from
the runner's `macos::open_offscreen` helper, which panics past a budget of 8
live windows and opens them hidden — set `KAGI_GUI_E2E_VISIBLE=1` to see them
for triage.

For Graph motion (#1617), scope `graph_selection_crossfade,graph_commit_insert_motion,graph_wip_insert_motion,graph_target_crossfade,graph_unborn_wip_motion,graph_activation_motion_reset,graph_mode_motion_reset,graph_insert_motion_hit_targets`.
These opt into the frozen motion clock; ordinary GUI scenarios remain instant.
Also run `graph_over_row_folder_drop,graph_stash_pointer_focus`: clicks must not suppress ancestor external drops or focus-on-mouse-down.
`graph_insert_motion_hit_targets` also right-clicks genuinely visible current-WIP/closing-ghost strips, checks real commit/stash/linked-worktree menus in compressed geometry, and drops a real external folder through the ghost at 100% and 167% zoom.
`graph_mode_motion_reset` hides genuinely mid-flight Graph behind PRs/Issues/Editor, accepts a hidden commit read, and returns at the same frozen time; the first visible frame must already be settled, while subsequent live WIP changes still animate.
The existing `KagiApp::hold_next_snapshot_for_e2e` seam holds the next reload
or tab activation/initial snapshot before backend work, only under `gui-e2e`.
`graph_activation_motion_reset` draws the cached/empty view while that real
read is held, then releases it and requires immediate final WIP presentation;
subsequent cheap live status changes must still animate.

The runner stops at the first failing scenario (exit 101). To collect every failure of a selection in one run, add `KAGI_GUI_E2E_KEEP_GOING=1` (it refuses to start without `KAGI_GUI_E2E_ONLY`): each scenario runs alone in a re-executed runner process with its own process group, one at a time; the whole group is killed after `KAGI_GUI_E2E_TIMEOUT_SECS` (default 600) and whatever it left running is killed when it ends.
The run ends with `[gui-e2e] KEEP_GOING summary: …` and one `PASS <name>` or `FAIL <name>: <exit status | timeout> — evidence <dir>` line per scenario (the dir also holds `exit.txt`), then exits 1 if any failed.
`KAGI_GUI_E2E_EXACT=<name>` is the internal seam the parent uses to start each child; do not set it by hand.

A change to how outcomes are presented (footer, toast, notice, modal, Operation
Log) must run the scenarios that assert that presentation with
`KAGI_GUI_E2E_ONLY` before merging. Compiling the GUI tests is not enough: #718
and #747 changed presentation, and three scenarios kept the old expectations
unnoticed until #898.

When a scenario panics, the runner writes failure evidence (#516) and prints
`[gui-e2e] FAIL <scenario>: evidence <dir>`. The directory is
`target/gui-e2e/<scenario>/` (worktree-local `target/`) and contains:

- `panic.txt`: the panic message.
- `klog-tail.txt`: the last 200 `[kagi]` lines.
- `fixture.txt`: `git status --short` and `git log --oneline -5` of every
  repository mounted through `mount` / `mount_state`.
- `window-<n>.png` for each window, or `window.txt` saying why there is none.
  Windows stay off-screen, and without Screen Recording permission
  `screencapture` cannot capture them.

Read these before re-running a decayed scenario. The next run of that scenario
deletes the directory first, so whatever is there belongs to the latest run.

After every scenario that passes, the runner compares shared state with what it
was before (#516 slice 3, `tests/support/gui_isolation.rs`). A difference fails
the scenario like an assertion, with the evidence above and a message naming each
resource and what it changed from and to.

What is compared:

- `settings.json`: every key and value, except `session_repos`,
  `session_active` and `recent_repos`.
- `worktree_ports.json`: byte for byte.
- `operations.jsonl`: the bytes already there must stay the same, and every
  appended entry's repo must be under the run's temporary directory or remote.
- Direct entries of the run's own `TMPDIR`: none may be added.

The run also owns `HOME` (and `XDG_CONFIG_HOME`): an empty directory under the
run root whose `.gitconfig` holds only the fixtures' identity (`poc`). Every
inherited `GIT_*` and `GH_*` (and `GITHUB_TOKEN` / `GITHUB_ENTERPRISE_TOKEN`)
is dropped at startup; then `GIT_CONFIG_GLOBAL` names that `.gitconfig`, there
is no system config, and `GIT_TERMINAL_PROMPT=0`. `git`, `gh` (config and
credentials), the editor's trash and the terminal never read the developer's
dotfiles or config. A terminal never starts the user's `$SHELL`: with no seam
shell set it panics before the spawn, failing the scenario.

Restoring state:

- `set_lang`, `set_theme`, `set_zoom`, `set_diff_split` and
  `set_terminal_auto_lock` save their key as well. A scenario that uses them
  holds `gui_isolation::SavedKeys::keep(&[...])`, which puts the saved keys back
  when it is dropped.
- A scenario that starts a terminal holds `gui_isolation::PortStore::keep()`,
  and `gui_isolation::StandInShell::install()` unless it sets its own seam shell
  (`KagiApp::set_terminal_shell_for_e2e`). The stand-in is a `/bin/sh` script
  that reads lines until `exit`, `exit N` (returning code N) or EOF; its PID and cwd are real, so the
  auto-lock cwd probe and exit delivery behave as with a login shell.

For #1273 use `KAGI_GUI_E2E_ONLY='terminal_states,terminal_auto_lock'`.
`terminal_states` measures NotStarted / Exited(3) / Failed(reason) icon, title,
hint and Primary Restart bounds in EN/JA at 100%/167%; Enter/Space and pointer
activation use the existing spawn seam. Failed starts target a nonexistent
fixture binary, never `$SHELL`; the OS reason and unchanged failed-start
Operation Log path are retained. Real tab round-trips preserve Exited/Failed
without another spawn; Restart alone retries. Native log-tail assertions require the
unchanged shell-exited line once, before the shell-wait status line. The
recorder proves renderer state, not real VoiceOver or Tier B focus delivery.
The wait-error leg uses `KagiApp::set_next_terminal_wait_error_for_e2e`:
the next spawned StandInShell is really waited/reaped before its result is
replaced by an I/O error. The presentation must become Failed with its reason
and recover through Restart; provenance stays Unknown, with no unlock offer
and no shell-exited log line. JA asserts localized `（終了コード N）` and
the existing `shell` spelling. The one-shot seam compiles only with gui-e2e.

For history bisection, strip repository-location variables exported by
`git bisect run` before launching a Git fixture. Otherwise a fixture's `git init`
can address the bisected repository instead of its temporary directory (#764).
Keep Cargo's worktree-local `target/` and the scenario filter:

```bash
KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY=preflight_presentation \
  git bisect run env -u GIT_DIR -u GIT_WORK_TREE -u GIT_COMMON_DIR \
    -u GIT_INDEX_FILE -u GIT_OBJECT_DIRECTORY -u GIT_ALTERNATE_OBJECT_DIRECTORIES \
    cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
```

The current suite covers:

- durable stash-drop recovery; history persistence; cleanup stale-tab,
  preflight, open-failure, and partial presentation; remove's public boundary;
  editor writer admission; commit-row and editor-history layout;
- custom theme loading (`KAGI_GUI_E2E_ONLY=theme_custom,theme_folder_controls`,
  `tests/recovery/theme_custom.rs`): #922 startup JSON load and slug restore,
  palette/menu/Settings selection and fallback, rejected-file toast, folder
  creation/open error, and a background reload held after parsing while a newer
  reload publishes; releasing the old read must not change colours, menus,
  selection, or toast IDs. A themes folder that cannot be listed (mode 000)
  keeps the loaded themes and the selection and names the folder in a toast.
  Startup loads before the window exists.
- system chrome accent (`KAGI_GUI_E2E_ONLY=system_accent_chrome`,
  `tests/recovery/system_accent.rs`): #1181 uses a scoped thread-local
  `theme::AccentInjection` to draw five detected colours under Apple Light/Dark
  without reading/writing the person's defaults. Dark with an unspecified
  source must keep yellow chrome; Light defaults to System. Explicit System
  primary/Input/custom keyboard-ring/graph-bar pixels match the detected colour;
  Theme accent restores the preset's fixed Blue/Yellow. Source changes keep
  control and label bounds fixed. Unit coverage includes all chromatic accents,
  graphite, label contrast, selection derivation and isolated settings migration.
  Tier B: Settings → Appearance shows the detected Mac name in EN/JA; compare
  System/Theme accent and return to Kagi after changing Appearance. No test
  command may write AppleAccentColor.
- file tree accessibility (`KAGI_GUI_E2E_ONLY=file_tree_roles`,
  `tests/recovery/file_tree_a11y.rs`): #354 Editor Workspace and Commit Panel
  render named EN/JA Trees with TreeItems carrying file status, selection,
  level and sibling position. Editor collapse hides descendants and updates
  `expanded`; Commit Panel generated-file disclosure exposes its children and
  updates positions. Reopening the same panel after changing both staged and
  unstaged file sets reuses its entity but recomputes TreeItem roles and sibling
  counts (the replacement state restarts its revision). A filename containing
  `{}` retains its literal braces in both languages. The recorder proves
  renderer attributes, not native VoiceOver delivery; verify the latter on a
  machine with VoiceOver enabled.
- remove refusal reasons (`KAGI_GUI_E2E_ONLY=remove_public_boundary`,
  `tests/recovery/app_remove.rs`): Enter on a locked removal plan delivers the
  specific EN/JA blocker and unlock guidance in AppNotice and bounded toast.
  One durable English refusal retains the reason; both repositories and the lock
  remain unchanged, and admission releases. Existing successful Enter/button
  removal legs still run. Tier B: right-click a locked linked worktree, open
  Remove, press Enter on the blocked plan, and inspect the notice in EN/JA.
- diff estimated extent (`KAGI_GUI_E2E_ONLY='pr_diff_extent,editor_diff_extent_unchanged_reload'`,
  #1122 / [ADR-0222](../../../docs/adr/0222-diff-list-estimated-height.md)):
  seven native scenarios use production reads and real wheel events, not installed
  test rows, hints or scroll offsets. `pr_diff_extent_unified` / `_split` assert
  initial global extent, one huge wheel reaching the true tail and return to the
  first row; `_reflow` asserts a fractional reading anchor across width changes;
  `_threads` asserts measured-node splicing, expansion geometry and collapse
  retiring expansion height; `_source_recolor` asserts equal-count source replacement
  invalidates heights while recoloring preserves identity and position;
  `_same_selection` asserts same-file re-click / reload retains the reading position;
  `editor_diff_extent_unchanged_reload` asserts one huge wheel reaches the actual
  Editor tail and unchanged reload retains source identity and position.
  Fork: `https://github.com/TomiXRM/zed`, rev
  `caf5007618c309a8ea66002d44a9c9ab629a2d35` (pending upstream).
  Rows that fit must be fully inside the viewport. An oversized tail must
  bottom-align within 1px, with its top above the viewport; an oversized first
  content row must return to native item 0 / offset 0, its hunk header top-aligned
  within 1px and its own top at header.bottom within 1px, extending below the viewport.
  A thread card taller than viewport.height − badge.height requires a fully visible
  badge, card.top ≥ badge.bottom − 1px, body.top inside the viewport and body inside
  the card; otherwise both card and badge must fully fit. Collapse removes one item,
  restores item 9 to code-row height (row 8 ±1px), and decreases extent by a positive
  amount at least expansion.height − viewport.height.
  Before for `_same_selection`: save a toggle patch reverting only the three
  `src/ui/pr_mode.rs` logic hunks (same-file / commit short-circuits and reload's
  `highlight::install` / adopt), **keep the recorders**, apply it with `git apply`,
  and run the exact same filtered oracle. The recorded Before fails at
  `tests/recovery/pr_diff_extent_same_selection.rs:260` on reading-position reset;
  restore with `git apply -R` and rerun unchanged (recorded PASS).
  The recorded patch was `/tmp/kagi-hig-audit-20261008/before-same-selection.patch`;
  recreate it from the named logic hunks if that local artifact is unavailable.
  Genuine Before failures also cover GPUI
  `test_estimated_end_resolves_taller_tail_without_following` and the Editor
  tail remaining outside the viewport after one huge wheel. Acceptance records
  GPUI unit 30/30 and all seven native oracles PASS; no FPS / CPU claim.
  Tier B real-app comparison is pending.
- refusal reasons (`KAGI_GUI_E2E_ONLY=refusal_reasons`,
  `tests/recovery/refusal_reasons.rs`): #353. For delete-branch (current
  branch, two blockers), push (no remote), pull (no upstream, the core's
  no-execute receipt) and stash push (clean tree, the stash app flow) in EN
  and JA, a real root Enter on the blocked plan closes the modal and leaves
  the first blocker's localized text — plus "+N more" — in the Failed footer
  and the Error toast, while the single durable entry keeps every blocker in
  English and the repository is unchanged. The `[kagi] footer:` contract line
  still reads `refused (N blockers)`. Tier B: open the same blocked plans in
  the real app, press Enter, and read the footer and toast in EN/JA.
- preflight refusal presentation (`KAGI_GUI_E2E_ONLY=preflight_presentation`,
  `tests/recovery/operations.rs`): stale stash-drop plans deliver the specific
  localized reason through a Failed footer and Error toast in EN/JA, with no
  dismiss-only AppNotice. The complete English blocker remains in one durable
  Refused receipt and its Operation Log panel row; stash list and repository
  fingerprint stay unchanged. History Undo retains its inline plan error.
  This follows the #747 delivery contract; #764 bisected the obsolete modal
  expectation to `e5644c6f`, rather than changing production back.
- unobservable remote-write release (`KAGI_GUI_E2E_ONLY=reconcile_unobservable_release,app_notice_modal_replacement`,
  `tests/recovery/reconcile_unobservable.rs`): measured native clicks exercise
  Inspect → arm → final confirmation in EN/JA. The armed warning is drawn;
  Escape and modal displacement discard the arm without releasing the requirement.
  Escape now dismisses the presentation until a fresh explicit write request
  (#1366); the scenario re-enters through a real Fetch admission refusal.
  A real append-lock failure preserves the requirement and writer exclusion,
  leaves no durable release row, and renders a Partial receipt rather than raw
  Unknown in the panel. Restoring persistence and confirming again records exactly
  one `reconcile-release-unobservable` audit row before admitting another write;
  the original Unknown and repository fingerprint stay unchanged. AppNotice uses
  `measure_inside` on the absolute modal root so instrumentation cannot relocate
  its controls outside the window.
  App-level tests in `app_unobservable_release_test` additionally reject live writers,
  mismatches, failed transports, unknown families, and failed audit persistence.
- actionable-notice dismissal (`KAGI_GUI_E2E_ONLY='app_notice_,alert_focus_notice,modal_,reconcile_unobservable_release'`,
  KEEP_GOING): `app_notice_escape_dismiss` and `app_notice_acknowledge_dismiss`
  exercise EN/JA Escape, five renders, opener focus, non-editing Return and fresh
  refusal recurrence. `app_notice_queue_recurrence` parks a real panicked checkout
  in a reconciling queue, dismisses its notice and explicitly enqueues Checkout
  through `e2e::enqueue_checkout`, which forwards to the unchanged production
  enqueue boundary (no manufactured queue state or mutation result). It checks
  active/queued Inspect/Acknowledge deduplication and five subsequent queue ticks
  without re-presentation. The native dispatcher assertions do not establish
  live VoiceOver or IME behavior; Tier B checks those and the restored opener.
- conflict Continue cache (`KAGI_GUI_E2E_ONLY=conflict_continue_cache`,
  `tests/recovery/conflict_continue_cache.rs`): real Result InputState changes
  cover omitted final LF, empty text versus a blank line, marker addition/removal,
  undo/redo and side selection, with an unchanged repository fingerprint.
  An unchanged parent repaint must not add an undo entry. The #497 temporary
  counter experiment instrumented both derived and fresh marker scans: each of
  five input changes scanned one file, then twenty unchanged native renders
  scanned zero. The counter was removed after acceptance; the behavioral
  scenario remains. Pair with `KAGI_GUI_E2E_ONLY=conflict_` for existing safety
  and ownership coverage; Save/Continue still perform fresh validation.
- conflict revision/draft authority (#1721):
  `KAGI_GUI_E2E_ONLY='conflict_draft_revision_,conflict_draft_inspection_,terminal_focus_exit,terminal_hide_focus,slow_write_conflict_continue,conflict_merge_failure_contract'`
  with KEEP_GOING covers fourteen native scenarios. Real two-conflict rebase
  consumers distinguish recorded and append-failed Continue, unchanged revision,
  unrelated index changes, saved stale/legacy drafts and repeated accepted reads.
  They enter a genuinely new Result, click Save, inspect its actual stage-0 blob,
  then Continue on the same long-lived Backend to verify the exact two replayed
  commit bodies. Do not reopen the Backend to hide an index-cache defect.
  The first real UI Continue pauses only after background completion and before
  foreground receipt/reload/adoption: its original durable bytes must still
  exist, and reopening Backend must expose the first body only as recovery for
  the actual second conflict. Release the one-shot hold through a scoped cleanup
  and continue normally; a backend-only consumer does not prove this UI boundary.
  Inspection cases reach the explicit preserved-draft action with zero receipts
  and after a real unrecorded restart; frozen attachment/generation and a replaced
  gitdir cannot deliver or copy another owner's body. Dedicated draft Copy and
  ordinary receipt Copy remain separate across Back and an open EN→JA→EN language
  switch; only the existing private test clipboard is used. The Terminal neighbors and
  real merge-Continue consumers cover the shared bottom-panel/staging routes.
  Add the narrow `KAGI_GUI_E2E_ONLY='conflict_continue_approval_'` selection for
  original Continue approval across a replaced same-step rebase, changed stage-0
  content, accepted mode replacement while the original modal remains open, and
  a new explicit plan after an unrelated index refresh. Save all conflicts first,
  retain the original confirmation, and use the existing `CONTINUE_CONFIRM_HOLD`
  to change real Git state after admission but before backend dispatch. Require
  the actual worker/completion boundary and a durable no-write refusal, not just
  a watcher dismissing the modal. Check HEAD, index, WT, sequencer and objects;
  only a fresh explicit plan/approval may advance the replacement state.
  Scoped cleanup must release any consumed hold and clear the existing confirm
  and completion slots even after failed assertions. No new production hold,
  controller or clipboard store is required; host clipboard operations remain
  forbidden.
  Pair with `cargo test -p kagi-git --test conflicts_test draft_` for exact known
  identity, unknown/legacy preservation, raw paths/modes, failed autosave,
  undo-to-null and two-live-buffer recency. `cargo test -p kagi-git --test
  conflicts_test continue_index` checks separately saved resolutions on a cached
  Backend, session-external unmerged paths, and malformed/directory index failures
  through planning and both public execute/stage boundaries. Add `conflicts_test
  continue_revision` for real external two-conflict advancement, same-operation
  text/raw stage changes, old payload plus new observation, unknown identity,
  unrelated index entries and fresh empty-session Continue. `conflicts_test
  draft_completion` reopens real Backend state before any UI adoption: Continue,
  Skip Advanced/Finished and merge completion must retain durable inspection-only
  bodies, never delete them before the foreground callback. These are native
  Tier A/backend checks, not plain-app Tier B evidence.
  Add focused `conflicts_test continue_metadata::`, `continue_operands::` and
  `continue_autostash::` consumers for changed rebase-merge / apply final-commit /
  MERGE_MSG / SQUASH_MSG, real nonempty retained input with changed author/date,
  zero-byte options consumed by a subsequent pending pick, and unreadable or
  nonregular metadata. Retain old plan/route/stage/execute and compare physical
  HEAD, index, WT, metadata and object counts before/after every stale request.
  Exercise packed ordinary reset/merge/pick targets, rewritten symbolic labels,
  pending operation-produced labels, processed update-ref CAS targets and
  squash-onto; unrelated ref movement must leave approval unchanged.
  Real merge/apply rebase autostashes must reject an old approval when the named
  target changes under unchanged file text, then apply the exact selected stash
  only under a new explicit approval. Root MERGE_AUTOSTASH coverage is conservative
  binding / symbolic rejection / Commit Panel staging, not Merge restoration.
  `cargo test -p kagi-git --test revision_test` retains canonical typed resolution.
  Do not call operation-state approval a freeze of refs/replace/config/hooks/exec
  dependencies or a guarantee of the eventual commit hash.

- repository-health proposal (`KAGI_GUI_E2E_ONLY=repo_health_proposal`,
  `tests/recovery/repo_health.rs`): #358 / ADR-0205. Analyze's Health axis
  reads a fixture without a commit-graph and suggests writing one; asking
  opens the shared plan card (equivalent command, no blockers) and nothing
  else — no commit-graph file, no receipt, unchanged repository fingerprint,
  also after Cancel. A real root Enter on the card writes it once (one
  durable `write-commit-graph` success) and the Health axis re-reads without
  the finding. Backend coverage (fsmonitor local-config-only, existing value
  left alone, unborn HEAD blocked, platform gate) is `tests/repo_health_test.rs`.
  Tier B: open Analyze → Health on a repository without a commit-graph and
  with `core.fsmonitor` unset, check the EN/JA texts, open each fix, read
  the plan card (equivalent command, recovery) and Cancel, then confirm one.
- Analyze accessibility (`KAGI_GUI_E2E_ONLY=analyze_a11y`, KEEP_GOING recommended,
  `tests/recovery/analyze_a11y.rs`): #1265–#1268 uses synthetic FileMetric data
  with the production bounded renderer, not a real Git mine. EN/JA, 100/167%,
  Apple Light/Dark, 940×660 and 1392×883 assert named selected Tab controls,
  actual arrow/Enter/Space dispatch, coupling expanded state, inert disabled Copy,
  model-matching risk text/AX descriptions, exact 150/300 render caps and a
  localized Status notice only above the cap. Help uses the single ActiveModal
  slot, named dialog, Tab/Shift+Tab trap, Escape and exact opener focus return.
  Attribute probes record where renderer semantics are installed; these are
  not native VoiceOver assertions. Keep `repo_health_proposal` and
  `ecosystem_evidence_` alongside it. The behavior-preserving shared TabList
  extraction additionally runs `commit_panel_segment_keys,editor_toggle_keys,home_,tabs_,workspace_mode`.
  Tier B: compare toolbar focus/disabled Copy, List/Map numeric risk and capped
  notices, coupling disclosure, and Help scroll/Escape/opener focus in EN/JA.
- text-first diff highlight (`KAGI_GUI_E2E_ONLY=diff_highlight`,
  `tests/recovery/diff_highlight.rs`): #495. WIP, Compare and File History
  diffs are read off the UI thread and first appear as unhighlighted text
  (an observer records the first shown state); the frame that renders them
  requests spans off-thread once. With split mode on, twenty unchanged frames
  and a reload that re-reads the same text add no highlight and no split
  projection and keep the same row allocation; a theme switch highlights once
  more. The stale leg leaves a highlight out while another file replaces it,
  moves the active theme under an outstanding request (the old theme's spans
  are never shown), and supersedes a WIP read by a newer commit diff or a
  close. `gui-e2e`-only counters (`diff_view::highlight::e2e`: highlight runs,
  dropped results, split projections) back the assertions; default builds do
  not compile them. Pair with `survives_reload,theme_switch,worktree_panel`
  for the reload / linked-worktree paths the same reads serve.
  `commit_diff_off_thread` (#829): a commit file's diff on a content-cache
  miss is read off the UI thread — nothing installs during the call, the
  shown rows keep their allocation until the read lands, a read superseded
  by another commit's cached file never lands or fills the cache, and a read
  held (`KagiApp::hold_next_main_diff_read_for_e2e`, `gui-e2e` only) across a
  reload that renumbers the rows survives the pane sweep, lands on its
  commit's new row and fills no cache under the stale row key.
- conflict refusal reasons (`KAGI_GUI_E2E_ONLY=conflict_save_boundary`,
  `tests/recovery/conflict_refusal.rs`): Save with remaining markers and Abort
  after an external staged resolution show the specific EN/JA reason in the
  AppNotice and bounded toast, retain the English footer contract and oplog
  detail, and leave the index, working file and merge state unchanged.
  Abort covers both the dashboard and the operation strip; both drift after the
  confirmation opens and confirm it *without* an intervening reload, because a
  reload now closes such a confirmation instead (#755): each leg reopens it,
  reloads, and requires the confirmation to be gone with no second
  `merge-abort` record and index / `MERGE_HEAD` unchanged.
- conflict Abort modal slot (`KAGI_GUI_E2E_ONLY=conflict_abort_escape_focus,conflict_abort_superseded_reload`,
  `tests/recovery/conflict_abort_slot.rs`): Escape closes the confirmation from
  the focus the app actually left behind, and only an *accepted* reload sweeps
  it — a superseded read leaves it standing, the next accepted one closes it,
  and neither runs the abort. The Escape half is the interesting one: the
  Result pane mounts its code editor only in Edit mode, so editing and
  returning to Preview leaves window focus on an
  element that is no longer drawn; gpui then dispatches from the tree root and
  every window action, Escape included, silently does nothing. The scenario
  reaches that state through the product's own toggle, opens the dashboard
  Abort, and delivers a real `escape` **without** re-focusing anything — unlike
  `press_key` in `tests/recovery/operations.rs`, which focuses `root_focus`
  first and therefore cannot see this class. Opening the confirmation takes
  focus back to the root, which is what makes Escape land.
  Tier B measured the same key on 2026-09-23 with `KAGI_NO_ACTIVATE=1` still
  set — a **non-key** window — and `pidclick key 53` closed the confirmation,
  so this check does not need the foreground. Do not read that as a guarantee
  that key delivery to a non-key window is reliable in general; it is what this
  window and this key did. `KAGI_DEBUG_KEYS=1` makes the routing wrapper print
  `[kagi] key: <key> char=<..>` (`src/ui/modal_key_routing.rs`), but **absence
  of that line is not absence of delivery**: gpui dispatches matched actions
  before key listeners and an action handler stops propagation, so a working
  Escape prints nothing. That is exactly what the accepted run recorded —
  no key line, modal closed. A printed `escape` with the modal still open is
  the interesting failure: the key arrived and no binding matched.
- Settings focus trap (`KAGI_GUI_E2E_ONLY=settings_focus_trap`,
  `tests/recovery/overlay_focus.rs`, #974):
  - Setup: a stand-in shell runs in the bottom-panel terminal, which holds
    the focus. Settings is then opened with `app.settings`, and the focus
    must be on the trap container.
  - Raw Tab: every press stays inside Settings. It walks into the
    Analyze-ignore editor and out again to Save without editing the text,
    and returns to the editor after one full turn. Shift+Tab also leaves
    the editor (#977).
  - Wrapping: from the container, Shift+Tab, Tab and Shift+Tab wrap around
    both ends while staying inside.
  - Close: Escape closes Settings and gives the focus back to the terminal.
  - Pointer open: a click on `tb-settings` lands the focus on the ring-less
    container. Escape then takes it out of Settings, back to the clicked
    button.
  - Toggle: from a focused terminal, Settings → raw Cmd+J closes both
    Settings and the bottom panel; Escape and several frames later focus
    remains on the visible root, not the hidden terminal. Repeat with the
    panel initially closed to verify the opening direction and with the
    command-id route used by the platform menu / palette. Removing the
    Settings-close helper from the shared toggle fails
    `settings-toggle-terminal: Cmd+J must close Settings`.
  - Checked mutations: without `.focus_trap` the wrap leaves Settings;
    without the focus on open, the focus stays on the terminal; without the
    return-focus capture, Escape does not return to the terminal; with the
    editor made an indenting multi-line input, Tab stays in it.
  - Unmounted return targets (`KAGI_GUI_E2E_ONLY=settings_hidden_return_target`):
    focus a sidebar mode-nav cell, open Settings through `app.settings`,
    press raw Cmd+B, then Escape *during* the 150 ms close animation,
    when the restored handle still belongs to the drawn root. Advance the
    stand-in clock past close; raw Down must reach Graph because
    `restored_focus` moves to root after unmount. Repeat with the inspector's
    selectable commit message focused and raw Cmd+Option+B hiding commit
    details, finishing the animation before Escape. Neither retained handle
    may receive keys after its pane disappears, even in the same session.
    Disabling either the return-time membership check or the per-frame
    `restored_focus` check must fail.
- Overlay z-order (`KAGI_GUI_E2E_ONLY=filter_menu_over_modal`,
  `tests/recovery/platform_menu_scroll.rs`, #976 review): open the Issues
  filter chip, then deliver the early Stash Push slot on a dirty tree. With
  root focused, Enter leaves the modal and repository fingerprint unchanged;
  Escape closes the filter first and retains the modal. `front_layer::Z_ORDER`
  drives both modal rendering and key ownership. Moving overlay menus below
  early modals must fail the front-menu assertion.
- overlay focus return (`KAGI_GUI_E2E_ONLY=palette_push_modal_keys,settings_close_returns_focus`,
  `tests/recovery/overlay_focus.rs`): #817 / #812. Every key is raw, with no
  test-side refocusing. The palette scenario first starts the bottom-panel
  terminal the way launch does — it takes focus, and Escape is bound
  `!Terminal` — then `cmd-p` → `push` → Enter opens the blocked Push plan:
  Enter refuses it (Failed footer, one durable Refused receipt) and a
  reopened plan closes on Escape. Settings is opened through `app.settings`,
  its theme picker clicked (the picker must hold focus), then closed by
  Escape and by ×; the next raw ↓/↑ must step File History. Tier B measured
  on 2026-10-01 that the terminal-focus case is what left Escape unmatched
  (`key: "escape"` printed, modal open) while Enter still reached the slot.
- Command Palette selection scroll (`KAGI_GUI_E2E_ONLY=palette_selection_scroll`,
  `tests/recovery/overlay_focus.rs`): #1069. Real arrow and wheel input checks
  the selected row's drawn bounds and preserves manual scroll until the next
  selection input. The gui-e2e-only scroll accessor observes the existing
  production `ScrollHandle`; it does not drive selection or replace the list.
  Pair with `palette_push_modal_keys,settings_close_returns_focus` for overlay
  ownership. Tier B: open Cmd+P, move beyond the first viewport, wheel away,
  then press Down and inspect the visible selection. Filter a custom-theme
  list with 100 entries in JA at 70% and 167%, inspect its last result, and
  check empty-result Enter and Escape/reopen. Use only safe View commands
  for live Enter verification; do not execute Pull/Push to test list scrolling.
- PR Peek visibility (`KAGI_GUI_E2E_ONLY=pr_peek_`,
  `tests/recovery/pr_peek_visible.rs`, #1102): four scenarios use actual table /
  sidebar menu actions, then the existing Compare file consumer and Main Diff.
  EN/JA at 100/167% cover hidden Inspector, an unloaded Graph head and an open
  Commit Panel whose entity/draft must survive. Dirty Editor Cancel, approval,
  tab departure, superseded intent and editor/input replacement preserve the
  correct owner and never discard through an obsolete approval. Missing fetched
  refs and a real object-database read failure keep the original PR context.
  Repository HEAD, staged index, working bytes, refs, stash and receipt counts
  remain unchanged; no fetch/checkout or replacement preview renderer is used.
  Tier B: Peek a fetched live PR from its actual menu, inspect visible Compare,
  open a changed file and use Back. Capture only the launched PID's window;
  a large fixture or sample is not proof of FPS/input latency.
- Settings' switches (`KAGI_GUI_E2E_ONLY=settings_switches`,
  `tests/recovery/settings_switches.rs`, #970): every switch is a
  `keyboard_nav::switch`. The scenario opens Settings through `app.settings`,
  checks each switch is named by its row (`e2e::recorded_switch`), walks Tab
  from the root over the six in drawing order
  (`settings_switch_focused_for_e2e`), presses Space then Enter on each of
  the five Appearance switches and checks the setting is saved each time and
  the checked state given to assistive technology follows (Smart Commit's
  is only reached: turning it on probes for local LLMs), and clicks one with
  the pointer (one flip). `SavedKeys` puts the five settings back. The ring
  is not observable in Tier A: Tier B Tabs through Settings and looks.
- Settings zoom bound (`KAGI_GUI_E2E_ONLY=settings_zoom_bound`,
  `tests/recovery/settings_switches.rs`, #1019 review): open the real Settings
  view with a legacy 150% value, then step from 156% to 167% and back to
  160%. The `gui-e2e`-only `e2e::settings_zoom_label` reads the
  rendered percentage after each frame; persisted `ui_zoom` must match.
  `GlobalSettings::capture()` restores the original settings. This Tier A
  probe cannot establish the label's physical appearance; Tier B remains
  unchanged.
- Tab panels (`KAGI_GUI_E2E_ONLY=tab_panels`, `tests/recovery/tab_panels.rs`,
  #979): the content a tab list switches goes through
  `tab_panel_a11y::tab_panel`, which records the role and label it set
  (`tab_panel_a11y::recorded_tab_panel`; cleared before each frame, so a
  frame that draws no panel reads `None`). Each workspace-mode nav mode
  (PRs, Issues, Graph) names `sidebar-mode-panel` a `TabPanel` after the
  mode; Branch Cleanup (a takeover, sidebar still drawn) gives it no panel;
  each Home pane names `home-pane-panel` after the pane, without the count.
  `repo_tab_panels` (#983, same file): with two repository tabs the
  workspace body (`repo-tab-panel`) is named after the tab in front, after
  opening the second and after switching back; with Home in front
  `home-tab-panel` is named Home and no `repo-tab-panel` is drawn.
  `gh` is a failing stand-in. What VoiceOver speaks is not observable by an
  agent (the #972 probe found the native AX tree exposes no GPUI content).
- Sidebar rows from the keyboard (`KAGI_GUI_E2E_ONLY=sidebar_rows`,
  `tests/recovery/sidebar_rows.rs`, #981): a fixture with branches alpha /
  beta / gamma, tags v1 / v2 and a linked worktree. ↓ from LOCAL's first row
  moves to the next and stops at LOCAL's last row (never into REMOTE or
  WORKTREES), ↑ stops at its first; TAGS collapsed by a click on its header,
  Tab from the window reaches that header (`sidebar_header_focused_for_e2e`)
  and Enter opens TAGS onto `tag:v1`; Enter on the worktree row opens the
  keyboard's inspection card (`sidebar-worktree-keyboard-card`), Escape
  closes it and the row keeps the focus. Rows are read and focused through
  `sidebar_row_focused_for_e2e` / `focus_sidebar_row_for_e2e` (keys
  `branch:` / `group:` / `remote:` / `tag:` / `worktree:<path>` /
  `stash:<n>`). From the #987 review: `view.toggleSidebar` with a row
  focused leaves no row focused and the root focused on that one drawn
  frame (no pump: a later frame would hide the bug), and Tab from the
  window reaches the open-but-empty REMOTE pane's header. Back from Issues
  to Graph in one update with one drawn frame, Tab (no frame between)
  still reaches a LOCAL row: what the panes drew when last shown is kept
  while they are away. `sidebar_rows_short` mounts a 300px-high window,
  where LOCAL has rows but draws none: Tab reaches LOCAL's header.
  `sidebar_rows_scroll` (40 branches): with b00 remembered, LOCAL scrolled
  to its end inside the one drawn frame still offers a drawn row to Tab (not
  b00); a focused b00 scrolled out has its focus on a drawn LOCAL row after
  the next frame. `sidebar_rows_keys`: a focused stash keeps the focus on
  its entry when a new stash pushes it down (keys are the stash commit), and
  a focused header of the empty REMOTE pane hands its focus to the row a
  refresh brings. `sidebar_rows_open_short`: with LOCAL's pane weight too
  small for a row (a short window; Tier A cannot resize), Enter on its
  collapsed header opens it and on no frame does an undrawn row take the
  focus — the header keeps it until the weight is restored, then the first
  row has it. `sidebar_rows_right_click`: a right-click on a focused row
  opens its menu and gives the window the focus, and the menu's first item
  takes it (#985), so Enter is the menu's, not the row's; Escape gives the
  focus back to the window, not to the row (dropping the right-click's move
  to the window fails it there). The ring is not observable in Tier A.
- modal input transitions (`KAGI_GUI_E2E_ONLY=remote_browse_escape_focus,pr_fields_escape_focus`,
  `tests/recovery/remote_browse_focus.rs`, `tests/recovery/pr_fields_focus.rs`):
  #755 follow-up. Real InputStates own focus before Remote Browse's
  Connect→Browse transition and PR Fields' Cancel/Apply exits. Raw Escape is
  delivered without test-side refocusing: it closes the browser or the notice
  queued behind the field picker. Both scenarios failed before the fix; PR
  Fields measured lost `CloseMainDiff` routing for both exits. The remote case
  also preserves input focus on failed and superseded connection results.
  Only the connect transport task is queued through
  `src/ui/remote_browse_e2e.rs`; validation, generation checks and completion
  remain production code. PR Fields uses a temporary offline `gh` that refuses
  Apply, so the real dispatch runs without a GitHub write. Its Cancel/Apply
  controls use the existing measured-button seam. Pair with
  `remote_browse_modal_routing,conflict_abort_escape_focus` for the earlier
  routing contracts. This follow-up uses Tier A; it does not claim Tier B or
  live SSH/GitHub coverage.
- bottom-panel placement and toggle (`KAGI_GUI_E2E_ONLY=bottom_panel`,
  `tests/gui_e2e_runner.rs`): the painted Terminal/Operation Log/Activity
  container occupies only the center workspace width, aligned with the commit
  list and stopping before the inspector; sidebar and inspector extend beside
  it to the window-wide status bar. Cmd-J hides it, the action restores it,
  and dragging its top edge changes its height without changing that alignment.
  Tier B: photograph the real window with a selected commit (visible right
  inspector), sidebar, and terminal panel to confirm actual pixels.
  `KAGI_GUI_E2E_ONLY=bottom_panel_nested`
  (`tests/recovery/bottom_panel.rs`) checks the PR and Issues navigators stay
  full-height next to the panel under their center panes, and Editor keeps its
  file tree/hunks beside the panel even after a child-only tree-width change.
  Tier B: also inspect the Editor and PR workspaces with the panel open.
  Graph copy, oplog expand/copy, snapshot creation, theme switching,
  agent provenance, and WIP-to-HEAD connectors retain their existing scenarios;
- right-pane Graph selection motion (`KAGI_GUI_E2E_ONLY=right_selection_motion,side_panel_motion`,
  `tests/recovery/side_panel_motion.rs`): opening Inspector reaches an intermediate
  width before its 180 ms endpoint; Esc closes through an intermediate width over
  150 ms. Deselect/reselect reverses from the current width. Reduced motion,
  tab switch and workspace-mode changes settle instantly; left/bottom pane
  contracts retain their existing scenario. Tier B: capture a real Graph window
  before selection, during the transition, fully open, then during Esc and closed.
- Graph header column dividers with the sidebar hidden
  (`KAGI_GUI_E2E_ONLY=graph_column_divider_hidden`,
  `tests/recovery/graph_column_divider.rs`, #1011): drag each divider 40px
  with the sidebar hidden and shown; both the saved column width and painted
  boundary move exactly 40px. Advance the sidebar-closing clock between drag
  events: after GPUI's drag threshold, each boundary follows the pointer even
  while the sidebar moves. Restoring the fixed sidebar-width offset must fail
  the hidden badge-column drag (-90px instead of +40px). Tier B: capture two
  real Kagi window screenshots, before and after the hidden-sidebar drag.
- WIP virtual commit anchors (`KAGI_GUI_E2E_ONLY=commit_row_layout_wip`,
  `tests/recovery/wip_layout.rs`, under the layout suite): actual canvas paints
  must show hollow nodes directly above their own HEADs (columns may repeat),
  with HEAD-coloured badge→node and node→HEAD dashes. Intervening history must
  not move an anchor beyond `abs(anchor_lane - head_lane) <= 1`; the selected
  policy uses the same lane. Node/dash `paint_order` proves that WIP paths are
  behind crossed nodes. The same run covers shared HEAD (stacked rings, one
  trace), detached/unborn state, 1.25× zoom, hover and selection.
  The same painted frame records `graph-commit-row` (29px at 100%) and
  `graph-commit-author` / `graph-commit-time` (96px / 48px at 100%,
  120px / 60px at 125%) using `e2e::measure_inside` on the first row/cells.
  `KAGI_GUI_E2E_ONLY=commit_list_roles` confirms the Graph AX row still
  names the full author and verbose age even though the visible time is short.
  For the compact metadata display (#1003), Tier B compares EN Dark
  screenshots of the same fixture before/after at 100% zoom and normal density.
  Ring diameter matches the visible HEAD ring (classic) or commit avatar disc
  (swimlane); stroke stays 2px, lane-coloured, without a fill. The trace is
  compile-time `gui-e2e` only and opt-in per window through
  `graph_view::take_paint_trace`; clear it before drawing and drain afterward.
  A measured `commit-list-viewport` wheel event must move every WIP offscreen
  while a visible HEAD retains dashes from the viewport top. WIP and stash
  prefixes share the commit list's virtual scrolling, not a pinned header.
  Pair with `wip_head_connector,worktree_wip_inline,worktree_panel_commit` for
  collision routing, linked-panel interactions and target-keyed row removal.
  Tier B uses three dirty worktrees on distinct branch tips, one extra branch
  commit above a WIP HEAD, and enough older history to scroll. Capture both
  the top and WIP-offscreen states; retain screenshots in the PR and verify
  that another pane does not obscure the graph.
- local squash ghost routing (`KAGI_GUI_E2E_ONLY=squash_graph_routing`,
  `tests/recovery/squash_graph_routing.rs`, #1627): a real wide-history fixture
  receives three links through the production scan callback. Check complete
  submitted endpoint move/line/curve commands, local bounds and intervening
  dashed segments in classic/avatar modes at 100%/167%, including overlapping
  and disjoint intervals and horizontal/vertical wheel input. Compare movement
  with actual nodes: GPUI's vertical-only list may also consume horizontal
  deltas. Keep real history, WIP ownership, read revision and publish generation
  unchanged; do not suppress scan scheduling. The compile-time `gui-e2e`
  `graph_view::record_graph_paints` guard arms endpoint recording for one window;
  drain `take_endpoint_paint_trace` around a real draw. Dropping the guard clears
  and disarms recording; draining alone does not arm it.
  Tier B: compare released/fixed windows on the same real patch-id squash
  fixture with old wide history and a locally reclaimed tip lane. Capture both
  styles and zooms, plus scrolling; retain screenshots in the PR and compare
  refs, staged content and worktree status before/after read-only interaction.
- linked-worktree WIP rows plus commit-panel commit, amend, and discard;
- Worktree occupancy and removal advisory (`KAGI_GUI_E2E_ONLY=worktree_inspection`,
  `tests/recovery/worktree_inspection.rs`): real pushed/dirty/locked/detached
  fixtures cover EN/JA reasons, ignored `target/` allocation, explicit refresh,
  the measuring spinner and late-delivery rejection after supersession,
  hovering another row and tab close. Bring virtual sidebar rows into view before
  hovering. The detail card appears for the hovered linked worktree only,
  leaves all five navigator panes their full layout height, and remains
  actionable when the pointer moves into it. Moving away hides the card;
  hovering another row changes its identity. Right-click a hovered row:
  the card yields to the existing worktree menu and its actions remain
  clickable. Check the name/path heading, independently scrolling body
  and Refresh action from inside the card in both languages.
  A partial-cache regression accepts one worktree report, switches to another
  repository and returns through the real tab lifecycle. Missing and newly added
  worktrees must then be measured while the completed report remains unchanged,
  even after its directory grows. Backend fixtures also reject mirror-refspec
  and symbolic-alias local refs as pushed evidence. Host unit tests cover Windows
  physical-size selection and unavailable-query errors; cross-checking the
  Windows module is not Windows runtime verification.
  Tier B uses those four states with allocated plus sparse build files and an
  ignored `.env`; inspect capacity/target breakdown, measurement time, localized
  reason, local-ref basis, `.gitignore` warning and existing removal-menu guidance.
  This is read-only: no new delete button, backup policy, lock operation or fetch.
  Unix fixtures compare allocation with `du`; a Windows compile check alone does
  not establish Windows runtime behavior.
- modal and branch-menu Enter isolation from the selected commit checkout;
- compact confirmation cards (`KAGI_GUI_E2E_ONLY=modal_compact,modal_sections`,
  `tests/recovery/modal_compact.rs`, `tests/recovery/layout.rs`): issue #462. The
  amend, discard-all and push cards are mounted at 1200x600/700/750/900
  in EN and JA, also covering 600px at 80% zoom — one window per viewport and locale, because
  `MacWindow::resize` never settles under `VisualTestAppContext`. Each card's
  measured `modal-card`, `modal-footer` and footer buttons
  (`amend-*`/`discard-*`/`plan-*`) must lie inside the window; the
  target rows must fit the actual scroller, with at least three visible at 600px
  and more than three at 700px. Real wheel events must expose the final file
  or commit, not merely a row near the end.
  The shared Push and bespoke Amend / Discard recoveries now start as one
  collapsed structured-command row at every height, never a prose scroll box;
  warning sections remain visible. An explicit recovery click survives a zoom
  change across compact/roomy, reopening restores the closed default. Both
  destructive cards are armed (first confirm click) and cancelled. Warning,
  blocker and armed text bounds must fit their visible body; recovery commands
  fit their own disclosure. A blocked plan retains a disabled confirm with
  the first blocker as its accessible description; its click leaves the
  fixture's HEAD + porcelain status unchanged. Git uses a local bare
  remote. `modal_sections` checks closed → open → closed and reset.
- Plan headings and recovery (`KAGI_GUI_E2E_ONLY=plan_card_heading,plan_heading_chipless,plan_recovery_noop,bespoke_plan_heading,plan_recovery_commands,plan_recovery_ax,plan_equivalent_summary,blocked_plan_command,dialog_a11y_roles,operation_strip_abort`,
  `tests/recovery/plan_card_994.rs`, `tests/recovery/operations.rs`,
  `tests/recovery/app_conflict.rs`, #994): EN/JA shared Push and Checkout
  measure an inline ≤20px icon, one-line short heading, optional typed
  target chip (no fallback chip when target is absent), and right-hand Copy all
  within the plan card while the AX dialog keeps the full title. Behind,
  approve / comment / request changes / set upstream intent, and file / stash / branch
  counts use localized units. CherryPick, Commit Plan and StashApply measure
  the inline icon and applicable target chip on their bespoke cards;
  CherryPick and StashApply also carry Copy all (#1023). Input-card headings
  remain out of scope.
  Ready, blocker-free Push with nonempty
  `PlanRecovery::commands_for(ShellKind::current())` has one initially
  collapsed `plan-recovery` row and command-only body; its dedicated Copy uses
  precisely those commands. Input cards apply the same Ready, valid-input
  and blocker-free gate to recovery commands. Copy all includes a separate
  commands block only for Ready plans with available commands; it and the
  dialog description retain the localized recovery explanation even when
  there are no commands.
  The reusable equivalent-command disclosure shows the bare command with a
  localized *equivalent* AX label; `plan_equivalent_summary` also checks that
  an identical recovery command prevents the duplicate disclosure. Detached
  Reset Current has neither executable-looking disclosure. Armed Conflict
  Abort and Sync to remote
  announce their stage first, then recovery; unarmed/single cards keep their
  previous action semantics. Four single-production-edit mutations failed
  their respective scenario before restoring a backed-up source file byte-for-byte:
  heading icon 18 → 40px failed `plan_card_heading`; disclosure default closed
  → open failed `plan_recovery_commands`; omitting shared-card AX recovery
  failed `plan_recovery_ax`; replacing the bare equivalent summary with the
  equivalent-label text failed `plan_equivalent_summary`. Returning CherryPick
  to the 40px badge failed `bespoke_plan_heading` before restoring the inline
  heading; the same scenario also renders Commit Plan and StashApply.
  Operation Log persistence and its ADR are follow-up #1025 (`Refs #994`).
  `plan_heading_chipless` opens the real Repo health → Write commit-graph plan:
  EN/JA have no fabricated chip, and restoring the `current.head[..8]`
  fallback makes it fail. `plan_recovery_noop` keeps a real push plan's
  nonempty recovery but changes its disposition to blocker-free NoOp at the
  modal seam: neither the card nor Copy all offers structured commands;
  changing the card gate from `plan_ready` to `blockers.is_empty()` makes it
  fail. `clipboard_text_offers_recovery_commands_only_for_ready_plans` fails
  the same mutation to Copy all's gate. All three mutations were restored
  byte-for-byte against their backups.
- CherryPick / StashApply bespoke recovery (#1023, `KAGI_GUI_E2E_ONLY=bespoke_recovery_1023`,
  `tests/recovery/plan_card_994.rs`): EN/JA Ready plans each show one closed command
  disclosure; the dedicated copy holds exact `commands_for(ShellKind::current())`
  while Copy all carries the recovery prose and separate command block. The
  AX dialog describes the recovery after its confirmation stage; the old prose
  box does not appear in the body. A blocker-free NoOp with commands does not
  expose the disclosure, and Copy all retains prose but omits the commands
  block. Mutating CherryPick's `plan_ready` gate to a blocker-only check failed
  the NoOp disclosure assertion; the original source was restored byte-for-byte
  against its backup (`cmp`).
- Dialog buttons (#1016 PR A, `KAGI_GUI_E2E_ONLY=create_branch_input_confirm_ime,repo_health_proposal`,
  `tests/recovery/operations.rs`, `tests/recovery/repo_health.rs`): a real card measures cancel and confirm at
  Small = 24px; a blocked confirm remains drawn and inert with its localized
  `aria_description` and disabled AX state. Destructive plans use the blocker
  colour (including two-stage armed confirmation); the plan-kind unit test
  covers the classification when native colour introspection is unavailable.
  Maintenance AFTER shows only a short localized state chip while AX and
  Copy all retain the full localized detail; Write commit-graph confirms with
  Write rather than Enable. Inputs remain Medium = 32px. Two production
  mutations failed independently: removing the helper's `.small()` made the
  blocked Create measure 32px in `create_branch_input_confirm_ime`, and
  mapping Prune to Repair in `plan_confirm_kind` failed the destructive
  variant unit test. Each source was restored byte-for-byte from a backup.
- Create Branch の成功後終了（#1092）は
  `KAGI_GUI_E2E_ONLY=create_branch_presents_backend_receipt,create_branch_input_confirm_ime,create_branch_execution_failure_keeps_input`
  （`tests/recovery/operations.rs`）。未送信の `feat` と同じ InputState が
  accepted reload 後も残り、unchecked の作成は HEAD を変えず、checked の作成は
  新 branch を checkout する。成功後は modal が消え、保持した旧 InputState から
  root へ focus が戻り、再 Enter は 2 件目の作成を記録しない。続く実キー
  Cmd+P / Escape が palette を開閉する（test 側で focus を戻さない）。
  実 ref `collision` を先に作り、blocker のない `collision/child` を Create
  ボタンで実行すると Failed 1 件となる。名前・同じ input・exact error は
  accepted reload 後も保持され、再読込は再実行せず、Escape で閉じる。
  receipt と Operation Log 行の ID 同一性は成功・失敗の両方で確認する。
  marked-text Enter は ref も receipt も作らず、unmark 後の input Enter だけが
  作成する。これは deterministic dispatch / 状態の検証であり、実 macOS IME と
  ウィンドウの focus の証拠は Tier B で別途取得する。Tier B は成功前後を同じ
  viewport / theme / language / zoom で撮影し、続く root shortcut、失敗時の
  名前・エラー保持、未送信 reload、IME 1 回目 / 2 回目の Enter を実入力で確認する。
- Input-confirm cards (`KAGI_GUI_E2E_ONLY=create_branch_input_confirm_ime,input_confirm_disabled_cards,stash_push_stacked_preview`,
  `tests/recovery/operations.rs`): #956, #1017. The real Create Branch and
  Stash Push cards measure `plan-state-current` above `plan-state-after`,
  aligned at the left; neither draws the horizontal arrow. The `-label` and
  `-head` probes verify 64px one-line labels, mono heads contained within the
  comparison, and a 47-character ref plus `branch: ` at MD width. Stash Push
  uses a dirty tracked file and an untracked file to verify its real plan has
  both status counts, a warning and a clean after state. The Group AX names
  use CURRENT / AFTER in EN and 現在 / 実行後 in JA.
  Four single-production-edit mutations were each run against
  `stash_push_stacked_preview` and failed before restoring the backed-up file
  byte-for-byte: comparison `flex_col` → `flex_row` fails the stack assertion;
  label width 64 → 44 fails the label-width assertion; AFTER → PREDICTED fails
  the AX prefix assertion; MD width 640 → 576 fails the 55-character head-room
  assertion. The restored scenario and the EN/JA `modal_compact` matrix pass;
  the latter also checks one-line labels at 1.5× zoom in the 900px window.
  `modal_widths_ordered` (#1022 review): the real Create Branch (MD) and
  Create Worktree (LG) comparisons are measured; LG ≥ MD and MD draws its full
  640px (comparison 604px inside border + padding). Putting the percentage
  window cap back for SM / LG fails it (LG drew 547px).
  Its ready and blocked actions are 24px high, and planning never writes to the repository.
  An empty name or a blocked plan renders no `input-recovery` row across all
  six cards. Set Upstream's invalid format appears below its input once, not
  again in the plan blocker list; a measured invalid-plan card catches duplicates.
  Create Branch has no recovery row even when ready. A ready Tag plan displays
  its structured Git command.
  A real marked-text Enter in ready Branch, Rename Branch and Set Upstream
  plans must leave the modal open and the repository unchanged; ordinary
  Enter after unmarking still creates the new branch. These are hidden native
  windows; for colors, glyph weight and chip legibility inspect Tier B.
- PR viewed files (`KAGI_GUI_E2E_ONLY=pr_viewed`, `tests/recovery/pr_viewed.rs`):
  #351 / ADR-0207. A real PR ref fetch (bare remote with `refs/pull/7/head`,
  reached through `url.<file>.insteadOf` for `github.com/example/repo`; other gh
  reads use `pr_fields_focus`'s offline gh) loads two files. Clicking each row's
  measured `pr-file-viewed-<n>` checkbox marks it without selecting the row, and
  the marks land in `pr-viewed/example-repo-7.json` as `{path: blob}`. Closing
  and reopening the tab reads them back from disk; a queued list response with a
  moved head whose new commit changes one file unviews that file only. The
  scenario advances the test clock before unmounting so gpui-component's
  Checkbox animation timers do not outlive the window (leak check). Tier B:
  open a PR, tick files, see `N / M viewed` and dimmed rows (EN/JA), close and
  reopen the tab, then push a commit changing one ticked file and refresh.
- PR review threads on the diff (`KAGI_GUI_E2E_ONLY=pr_threads`,
  `tests/recovery/pr_threads.rs`): #351 / ADR-0209. The `pr_viewed` fixture's
  real PR ref fetch loads `c.txt` (line 2 rewritten); the queued conversation
  carries RIGHT line 2, LEFT line 2 and an outdated RIGHT thread on line 3.
  Unified: badges `pr-thread-badge-<row>` on exactly those rows; a real click
  opens `pr-thread-<row>-<k>` directly under its row with its body
  (`pr-thread-body-…`) laid out and the next row pushed below; the outdated
  thread opens as `pr-thread-outdated-<row>-<k>`; closing both returns the list
  to one item per diff row. Split: `-l` / `-r` badges in the left / right
  gutter, and a RIGHT thread on a context row only on the right.
  `pr_threads_via_gh` (same substring) goes through the production read path: a
  fake `gh` on `PATH` answers two threads only if every field the parser reads
  is requested by its own name (else GraphQL `errors`, exit 1, as GitHub), and
  the tab must hold both; a failed read logs `[kagi] pr-threads: #N read failed:`.
  For #1072 the same filter also runs both scenarios in
  `tests/recovery/pr_threads_projection.rs`: same-count source replacement,
  current body/side replacement, unified/split and file changes, wheel/open/close
  reading anchors, and separate PrKey/repository-session owners. Pair with the
  complete `pr_diff_extent_` group plus `diff_keyboard_selection,diff_hunk_buttons`;
  offscreen expanded-body remeasurement must remain enabled.
  Performance Tier B uses a 20,000-plus-line file with current inline anchors
  and a similarly sized unanchored control in both modes. Keep the same build
  profile, viewport, zoom and gesture sequence. Record actual diff-row count
  separately from source-file length. Temporary synchronous construction probes
  must cover both header and diff stages, then be removed before final gates;
  their timings are not FPS, paint time or input-to-display latency.
  Tier B: open a
  PR with line comments (current and outdated), click badges in unified and
  side-by-side, check the dimmed outdated card, EN/JA chips, no resolve button,
  and that scrolling does not jump when a thread opens or closes.
  For #1660 add
  `KAGI_GUI_E2E_ONLY='main_diff_reading_context,file_history_reading_context,editor_diff_reading_context,pr_threads_mode_anchor_'`.
  These six scenarios exercise same-source Unified/Split reading anchors,
  unequal/one-sided pairs, wrapping, settled tails, open/offscreen LEFT/RIGHT
  threads, code-only Copy/focus, and different-file/same-count source resets.
  Main Diff also checks a noninitial hunk header through its rendered action.
  Main Diff and File History click measured header segments; Editor and PR
  use the existing mode setting plus native paint because their headers have
  no bounds recorder. Do not describe those two drivers as actual header clicks.
  Tier B must click their real controls too, comparing the same source before
  and after both mode directions, including EN/JA, normal/high zoom,
  Apple Light/Dark and narrow windows. Split pairs choose their left source
  representative (right when absent); combined expansions choose their first
  open source row. This is item/fraction retention, not exact sub-card pixels.
- PR suggestion apply (`KAGI_GUI_E2E_ONLY=pr_suggestion_apply`,
  `tests/recovery/pr_suggestion_apply.rs`): #351 / ADR-0210. The same real PR
  ref fetch as `pr_viewed`, with the PR branch checked out so `s.txt` is the
  head's blob, and an injected line comment carrying a ```suggestion block.
  Clicking the measured `pr-convo-apply-suggestion-7000` on the Review page
  opens the plan card with no blocker and writes nothing; `plan-cancel` closes
  it with the file and the oplog unchanged. A second click and `plan-confirm`
  rewrite exactly line 2, stage nothing, and record one successful
  `apply-suggestion` entry with one `refs/kagi/backups/` ref. Blob mismatch,
  missing head, TOCTOU and gc recovery are `crates/kagi-git/tests/
  suggestion_apply_test.rs`. Tier B: open a PR with a suggestion on its
  checked-out branch, apply it (EN/JA card), then edit the file and see the
  "not the PR head's version" refusal.
- header buttons stay reachable (`KAGI_GUI_E2E_ONLY=header_fit`,
  `tests/recovery/header_fit.rs`): #809. A real `KagiApp` at 1000x720, zoom
  1.25, EN and JA opens a commit's long-path file diff, then File History for
  it. Each Main Diff header control (`main-diff-back`, `-ext-editor`,
  `-history`, `diff-mode-unified`, `diff-mode-split`, `main-diff-stats`), each File History header
  button (`fh-back`, `fh-refresh`, `fh-copy-path`, `fh-open-file`,
  `fh-follow`) and the embedded diff's mode segments/stats must lie inside its header
  row, and each row inside the window — bounds recorded by the production
  `HeaderFit`, which is what the pane crate can reach. The narrow case must be
  icon-only; 1920x1080 at zoom 1 must keep the labels. Tier B: 1000px wide at
  125% in JA, open a file diff and File History; every button is visible and
  clickable, icon-only buttons name themselves in a hover tooltip, and a wide
  window shows the labels again.
- diff presentation (`KAGI_GUI_E2E_ONLY=diff_mode_segment,diff_binary_states`,
  `tests/recovery/diff_presentation.rs`): #1235/#1236. The first scenario clicks
  measured Unified/Split segments and checks Button role/name/current selection;
  pair it with `header_fit` for narrow icon-only collapse. The second renders
  binary, added, modified and deleted images in EN/JA using production reads.
  `conflict_save_boundary` also renders binary conflict sides in both languages
  and checks the shared no-text-diff explanation. A partially previewable
  modified image retains both versions and shows a preview-unavailable side,
  rather than incorrectly claiming it was added/deleted; preparation unit
  coverage includes BMP on either side. Tier B: compare selected paint, focus
  and icon tooltips in narrow/wide diff and File History panes, then inspect
  each binary/image state and the existing external-editor action.
- worktree port in the shell and the sidebar (`KAGI_GUI_E2E_ONLY=worktree_port_env`,
  `tests/recovery/worktree_ports.rs`): #855 / ADR-0171. The shell is swapped for a
  script through `KagiApp::set_terminal_shell_for_e2e` (`gui-e2e` only) that
  writes the `KAGI_PORT` / `KAGI_WORKTREE_PATH` it was spawned with, so the
  oracle is the real PTY child's environment: it must equal the block the store
  holds for the worktree. The main worktree still owns its port block but has
  no sidebar row; a linked worktree assigned before mount shows its block
  (read by the snapshot), and one never assigned shows none and stays
  unassigned. In nonconcurrent mode the linked row displays the main block.
  Tier B: open a linked worktree terminal, see `localhost:<port>` on its row,
  click it and check the browser opens that URL.
- nonconcurrent run mode (`KAGI_GUI_E2E_ONLY=worktree_nonconcurrent`,
  `tests/recovery/worktree_ports.rs`): #859 / ADR-0213. With
  `worktree_run_mode` = `nonconcurrent` and the shell seam swapped for `exec cat`,
  the linked worktree's tab starts a shell; the main worktree's tab of the same
  repository then spawns none and the footer names the running worktree. EOF to
  the first shell (its `cat` exits, observed by the #772 wait) lets the main tab
  start. With the setting removed (default `concurrent`) the linked tab starts
  alongside. The decision is unit-tested in `kagi_domain::worktree_run_mode`.
  #869: while the linked shell runs in `nonconcurrent` mode, the `side`
  sidebar row carries the main worktree's stored port block; main has no
  sidebar row, and `side` has no store entry. The env side is covered by `kagi-git`
  `nonconcurrent_hands_every_worktree_the_main_block`.
  Tier B: set the key, open terminals in two worktrees of one repository, read
  the refusal (EN/JA), `exit` the first, then open the second.
- remove a worktree with a live terminal shell
  (`KAGI_GUI_E2E_ONLY=worktree_remove_live_shell`,
  `tests/recovery/worktree_remove_shell.rs`): #867 / ADR-0171 Follow-up 1.
  - Setup: the shell seam runs a script that starts `nohup sleep 3094 &` and
    then `exec cat`. The linked worktree's tab starts it.
  - With the shell running, the remove plan from the main tab carries the
    `RemoveLiveShell` blocker. Its text names the worktree and says `exit`.
    Confirming refuses (the notice repeats the reason) and the directory stays.
  - After EOF to the shell (exit observed): no blocker, and a
    `RemoveLeftoverProcesses` warning with count 1 for the `nohup` job.
  - After `pkill` of the job: no warning, and confirming removes the worktree.
  - Unit tests cover the decision (`kagi_domain::worktree_remove_shells`) and
    the session probe (`kagi_git::proc::session`).
  - Tier B: open a worktree's terminal, run `nohup sleep 999 &`, try to remove
    it (blocked, EN/JA), `exit`, then plan again (warning) and remove.
- Add Worktree のフォルダ選択 (#1043):
  `KAGI_GUI_E2E_KEEP_GOING=1 KAGI_GUI_E2E_ONLY='worktree_folder_picker'`
  (`tests/recovery/worktree_folder_picker.rs`)。native dialog は runner から操作できないため、`e2e::answer_folder_prompt` で答えを決め、`e2e::take_folder_prompts` で要求を数える。EN/JA で、パス入力の左にある 24px のボタン(AX 名「Choose folder / フォルダを選択」)と、32px のままの入力欄を確認する。キャンセルではパスが変わらない。フォルダを選ぶと `<選んだフォルダ>/<branch 名>` が入力され、手入力と同じく計画され、何も書き込まれない。
  mutation で FAIL することを確認済み: 選んだフォルダをそのまま使う、選択を反映しない、ボタンを右に置く、28px にする。Tier B は隔離した実アプリで Add Worktree を開き、ボタンをクリックして Finder のダイアログが開くこと、選んだ結果が入力欄と CURRENT / AFTER に反映されることを確認する。
- Operation Log 展開パネルの選択とコピー (#1053):
  `KAGI_GUI_E2E_KEEP_GOING=1 KAGI_GUI_E2E_ONLY='oplog_detail_select_copy'`
  (`tests/recovery/oplog_select.rs`)。実 `run_recorded` の create-branch 行を要約のクリックで開き、detail の文字から記録した ref の文字までドラッグして `WindowExt::selected_text` が空でないこと、⌘C で clipboard に before / after・復旧コマンド・ref 名が入ること、ドラッグと展開部分のクリックで行が閉じないこと、取り消すボタンで計画カードが開くことを確認する。selection は最後に描いた hover で当たり判定するため、押す前に move と描画が要り、両端は文字の上に置く。
  mutation で FAIL することを確認済み: detail の `on_mouse_down` で propagation を止める、開閉の click を行全体に戻す、記録した ref を普通の div に戻す。Tier B は隔離した実アプリで branch を作り、Operation Log の行を開いてボタンの見た目を撮影する(pidclick に drag はないので選択は Tier A で担保)。
- Operation Log の計画復旧説明 (#1025 / ADR-0221):
  `KAGI_GUI_E2E_KEEP_GOING=1 KAGI_GUI_E2E_ONLY='oplog_recovery_,oplog_actor_reflog,oplog_restore_card,oplog_expand_copy'`
  (`tests/recovery/oplog_panel.rs`)。実 `run_recorded` の Success 行で EN/JA の説明、重複しない英日別ラベルのコマンド行、AX Recovery group と Copy を確認する。実行前に HEAD を動かした Failed 行には Recovery 欄を描かず、旧形式の Success 行は「not recorded / 記録されていません」を表示する。
  backend は `cargo test -p kagi-git recovery_plan` と `cargo test -p kagi-git --test oplog_serde_compat_test`、保持中の行の byte identity は `retirement_matches_typed_recovery_and_keeps_other_line_bytes`。Tier B は隔離した実アプリで新しい branch を作り、Operation Log の該当行を開いて EN/JA と Copy の説明・コマンドを撮影・確認する。古い行には推測の手順を表示しない。
- Operation Log badges, recorded ref moves and estimated reflog
  (`KAGI_GUI_E2E_ONLY=oplog_actor_reflog`, `tests/recovery/oplog_panel.rs`):
  #334 slice 1 + 2a / ADR-0214. Three real `Backend::run` writes as Human / MCP /
  CLI, one second apart, become three rows with painted actor and worktree
  badges. A real click on the checkout row paints the **recorded** HEAD move
  (main → oplog-a) and no estimate, and starts no reflog read; the first
  creation paints its recorded `refs/heads/oplog-a` creation. Two old-format
  entries (no `ref_moves`) of another worktree that share one second with a
  reflog line show the **estimate**, that line marked ambiguous. The repo
  fingerprint and the oplog length are unchanged (reads only). Recording:
  `crates/kagi-git/tests/oplog_ref_moves_test.rs` (checkout / commit /
  replay-onto / failed op / a move outside the pipeline); codec compatibility:
  `kagi-git` `ref_moves_distinguish_not_recorded_from_nothing_moved`; window:
  `crates/kagi-git/tests/oplog_reflog_test.rs`. Tier B: open the Operation Log
  tab after a few operations (and one through `kagi` CLI / MCP), check the
  badges, select a new row ("Recorded") and one recorded before this change
  ("Estimated").
- Operation Log revert / restore card (`KAGI_GUI_E2E_ONLY=oplog_restore_card`,
  `tests/recovery/oplog_panel.rs`): #334 slice 2b-2 / ADR-0214 §5. Three real
  `create-branch` runs (keep, drop1, drop2) and one unrecorded entry of another
  repository. The unrecorded row's "Revert this operation…" / "Restore to this
  point…" are both painted disabled. A real click on the first creation's
  "Restore to this point…" opens the card: `Moves` deletions of drop1 / drop2,
  `RefsOnly`, `plan-confirm` drawn, branches unchanged. Enter arms (nothing
  moves), Enter again restores (keep + main left, newest entry
  `restore-to-point`); the restore's own row's "Revert this operation…" puts
  drop1 / drop2 back (`op-revert`). Backend: `crates/kagi-git/tests/
  oplog_restore_test.rs`. Tier B: in a scratch repository, create a few
  branches, select an earlier row, read the card (EN/JA), restore, then revert
  the restore from its row. #334 slice 2c / ADR-0214 §6: a recorded commit on
  main is added first, so the card also draws the graph after
  (`restore-preview`): main's moved label on its target commit, 1 commit off
  every branch (`restore-preview-removed-1`, equal to the drop in `git
  rev-list --count --branches` after confirming); the revert card's target is
  no longer loaded, so it paints `restore-preview-unavailable`. Domain rules:
  `kagi-domain` `restore_preview`. Tier B: read the graph after on the card
  (EN/JA) before confirming.
  #988 extends the existing scenario: a ref-only blocked plan has no
  Confirm button or accessibility Confirm action; REFS rows and the
  CURRENT-to-AFTER graph stay visible without re-laying the original rails.
  The checked-out branch's warning and six unchanged-state chips remain
  outside the graph; Copy all includes the ref transaction and preview.
  At the first confirmation no ref moves; at the second the existing
  `run_recorded` pipeline performs the write.
- Blocked plan command (`KAGI_GUI_E2E_ONLY=blocked_plan_command`,
  `tests/recovery/operations.rs`): #993 P2. A real detached-HEAD Reset Current
  plan has a blocker and still carries `git reset --soft` in the backend.
  The shared confirmation renders neither the collapsed command nor its Copy
  button; Copy all keeps blocker details but not the command, and inspecting it
  leaves the repository unchanged. The shared visibility predicate also gates
  Operation Log restore; `oplog_restore_guarded_rows` covers that card.
- Restore card safety and long refs (`KAGI_GUI_E2E_ONLY=oplog_restore_guarded_rows`,
  `tests/recovery/oplog_panel.rs`): #993 review. Reverting a recorded
  `create-branch` whose branch is now checked out yields a real
  `DeletesCheckedOutBranch` blocker while the backend still carries an
  equivalent `git update-ref --stdin` command. The card hides the command
  disclosure and dedicated Copy button; Copy all retains the ref details but
  omits the executable command. Enter records a Refused receipt without
  changing the repository. A second fixture moves a >200-character branch
  through a recorded commit; restoring to its creation leaves both expected
  and destination OID bounds inside the row/card, bounds the name chip, and
  exposes its complete canonical ref in AX and Copy all. Inspection/copy
  leaves repository fingerprint and oplog unchanged.
- Operation Log local-tag restore (`KAGI_GUI_E2E_ONLY=oplog_restore_tag_preview`,
  `tests/recovery/oplog_panel.rs`): #887. A recorded branch point followed by
  a recorded local tag opens Restore to this point. The card lists the tag
  deletion, paints a neutral unavailable graph preview rather than predicting
  commit rows, and two confirmations delete only that tag. Backend round trips
  lightweight and annotated tags through their raw OIDs, with CAS drift
  refusal and retained annotated objects in
  `crates/kagi-git/tests/oplog_restore_test.rs`. Tier B: create a local tag
  after an earlier recorded operation, inspect the card and its neutral
  preview, then confirm and check that only the local tag disappeared.
  #988 adds a malformed canonical plan-row admission leg to the same
  scenario: no card opens or ref moves, the full error persists in a Failed
  receipt, and the footer/toast carry only a bounded EN/JA preview.
  #993 review also checks the EN/JA missing-session footer and the no-repository
  toast, without appending another receipt or moving any ref.
  `oplog_restore_preview_review` uses a long recorded branch history with
  Solo enabled and a fetched PR head as a fixed root. The ghost count omits
  commits that PR ref retains; the six-row AFTER window scrolls to the last
  loaded row, and Copy all contains its projection. Scope it explicitly with
  `KAGI_GUI_E2E_ONLY=oplog_restore_preview_review`.
- Operation Log restore across a resolved merge
  (`KAGI_GUI_E2E_ONLY=oplog_restore_across_merge`, `tests/recovery/oplog_panel.rs`):
  #884 / ADR-0214 §4. `create-branch mark` → `merge-into-conflict side` → the
  production `run_recorded_conflict` save → `merge-commit`; the real "Restore
  to this point…" on mark's row opens a card with no blockers, and two
  confirms put main back before the merge. `stash_conflict_close_reopen` also
  asserts the UI continue's persisted entry records `ref_moves = Some([])`.
  Backend: `crates/kagi-git/tests/oplog_conflict_ref_moves_test.rs`. Tier B:
  merge a conflicting branch, resolve and commit in Kagi, then restore to the
  row before the merge.
- terminal auto-lock compare-and-unlock (`KAGI_GUI_E2E_ONLY=terminal_auto_lock_race`,
  `tests/recovery/worktree_lock_reason.rs`): #836 / ADR-0212. (b) Through the
  Backend race seam (`execute_auto_unlock_worktree_racing`,
  `AutoUnlockRace::RelockBeforeMove`), a lock replaced between preflight and the
  release is put back and refused, and the other reason remains. (c) A hand-placed
  `locked.kagi-*` leftover shows as `LockLeftover` on the real manual unlock card
  and blocks the auto release; Enter records a refusal and leaves the lock and the
  leftover. Backend cases (normal release, relock after the move kept, no-clobber
  restore leaving a leftover, idempotent) are `crates/kagi-git/tests/
  worktree_autolock_test.rs`. Tier B: Phase 1's steps, plus a `locked.kagi-*` file
  placed by hand in `.git/worktrees/<name>/` before opening the unlock card.
- external lock on refresh (`KAGI_GUI_E2E_ONLY=external_lock_reload`,
  `tests/recovery/worktree_lock_reason.rs`): #851. Opened at the linked
  worktree and at main, a `git worktree lock` made outside Kagi after launch,
  then the real Cmd+R (`file.refresh` = manual reload + quiet fetch): the read
  model, the painted sidebar row (🔐) and the row's right-click menu all report
  the lock. The fetch's admission supersedes the refresh's read; a no-op fetch
  must re-issue it. The inspection column (Keep/Unknown) is not part of this —
  a new read makes cached observations stale by design until re-measured.
- worktree lock reason (`KAGI_GUI_E2E_ONLY=worktree_lock_reason`,
  `tests/recovery/worktree_lock_reason.rs`): #372 item2. EN/JA use the real
  InputState, clipboard paste and focused Enter to review, then a second Enter
  to lock without test-forced refocusing. Input/plan cancellation makes no lock;
  Unicode/quotes and blank reasons round-trip through Git's NUL-delimited
  porcelain, with a durable Success and unchanged HEAD/working tree. The existing
  explicit unlock is exercised between acquisitions. For Tier B, right-click a
  linked worktree → Lock, edit the reason → Review → inspect the plan, then
  Cancel or confirm. No tab-strip click or foreground activation is required.
  Automatic terminal locking is not part of this change; it is tracked in #772.
- Home tab (`KAGI_GUI_E2E_ONLY=home_tab,welcome_startup_renders,close_last_tab_welcome_renders`,
  `tests/recovery/home_tab.rs`, #923 / ADR-0219): the tab strip's `+` (`tab-add`)
  and New Tab (⌘T) put Home in front (repo commands off, ⌘W closes Home only); a
  recent row (`home-recent-N`) turns Home into that repository's tab, an already
  open one is switched to (no duplicate); clicking a repository tab
  (`repo-tab-N`) only moves Home to the back and its tab (`home-tab`) brings it
  back; "Connect to SSH remote…" opens Remote Browse over Home; with no tab Home
  is the whole window. Home ends the covered tab's visit: a merge plan started
  just before ⌘T is dropped (no modal), and closing Home re-enters that tab
  (`panes_revalidating`). Home draws the workspace's modal layer: an AppNotice
  (`active-modal/app-notice`), a Pull confirmation (`plan-cancel`) and the
  Editor unsaved-changes guard raised by closing the dirty tab behind Home
  (`active-modal/editor-dirty-guard`; Esc keeps the tab) are on screen, as is
  Settings (`settings-theme-select`). Closing the covered tab does not enter
  its neighbour behind Home. `recent_repos` is restored by `SavedKeys`. For Tier B
  press ⌘T (no foreground needed) or click `+` (needs a key window — ask first);
  "Open Folder…" opens the native dialog, which Tier A does not click.
- Home's GitHub list and clone (`KAGI_GUI_E2E_ONLY=home_github`,
  `tests/recovery/home_github.rs`, #923 #924 #930 review / ADR-0219): the
  stand-in `gh` fails on marker files in a state folder and answers
  `config get user` with `acme` (or `state/user`). A saved list
  (`github_repos_cache.json` beside `settings.json`) is read off the UI
  thread and shown only for the account it was saved as (`github.com/acme`;
  one saved as another account is not), and stays when the own-list refresh
  fails (toast names the 502); after switching the user the shown list is
  dropped; a
  second Refresh while one runs joins it (`own-calls` counts one read); a
  Refresh keeps the list drawn beside the `home-github-updating` mark; a failed
  `gh api user/orgs` is the `home-github-orgs-failed` row and is not saved
  over the full list. An offline `gh` lists
  the user's repositories (one whose `origin` matches a recent repository, one
  that is not local) and two organizations — one listed, one refusing with a
  SAML error, which keeps its section with the reason; `repo clone` really
  clones a local bare repository. The organization's row opens a clone card
  sourced from that owner. The local row (`home-gh-<owner>/<repo>`) opens its
  tab; the other opens the clone card with no folder chosen (Kagi never picks
  one), where `clone-confirm` does nothing. The folder is supplied through
  `replan_clone` — what the native dialog's callback calls; a folder whose
  `widgets` is occupied is refused (the file in it untouched), and once free
  Clone clones, writes one `clone` Success receipt keyed by the destination
  and opens the clone in place of Home; while it runs `may_close_host()` is
  false (⌘Q / window close are held) and true again once it is recorded.
  The card sent to the background while the clone runs comes back, still
  running (`started`), when the row ("Cloning…") is picked again (#944).
  Before that a clone into another
  folder fails (`fail-clone`): Home stays in front with a `clone: failed`
  toast drawn there (`toast-stack`). Back on Home, the clone's tab is matched
  into `home_github.local` though it was opened after the list was read, and
  its row's chip is drawn as `home-gh-acme/widgets:Open`. The list's entries
  are built once per change of what they come from (#937): three redraws
  leave `e2e::home_item_builds()` unchanged, a new filter builds them again.
  `home_list_place` (`tests/recovery/home_list_place.rs`, #942 review) lists
  60 repositories, scrolls to entry 30 and checks it stays there — without a
  rebuild — when the hidden pull request / issue lists land
  (`reload_home_work_for_e2e`), and in place but rebuilt when the language
  changes and when a Refresh lands (`home_list_top_for_e2e`).
  `recent_repos` is restored by
  `SavedKeys`; `home_tab` also installs an offline `gh` because Home reads
  `gh repo list` whenever it opens, and checks that Remote Browse opened from
  Home is drawn centred over it (`remote-browse-card`). For Tier B use a real
  `gh` login, choose a temporary folder with "Choose…" (the native dialog)
  and check `[kagi] clone: done … ok=true` and the receipt.
- Home's pull requests and issues (`KAGI_GUI_E2E_ONLY=home_work`,
  `tests/recovery/home_work.rs`, #928 / ADR-0219 decision 8): an offline
  `gh` answers the three `gh search`es (a PR and an issue of a repository
  cloned locally, a draft PR of one that is not, a review request) and
  `pr view -R`; marker files fail every search (`fail-search`) or the review
  search (`fail-review`), and `state/user` switches the `gh` account. The
  test dispatcher runs `gh` inside its pump, so Home is opened and drawn
  before the pump: the switch draws `home-pane-prs-spinner` (never a count of
  0) and the pane `home-work-loading`. Lists saved as another account are not
  shown; after the account switch that account's saved lists are, and stay
  when its searches fail. Rows are `home-work-<owner>/<repo>-<N>`. A failed search draws
  `home-work-failed` above the list read before and saves nothing; the
  non-local row opens on GitHub (the URL is recorded, `e2e::take_opened_urls`),
  stays focused, and Enter / Space press it again (#944; the scenario sends
  each key's release, on which gpui's keyboard click fires),
  and every row's `-open` end opens on GitHub without the row's click; the
  local PR row draws its `-opening` spinner while `pr view` runs, ignores a
  second click (one `view-calls`), then opens PR #7 in the clone's tab. A
  non-local row picked while that local PR is still opening wins: it opens on
  GitHub, and Home stays in front when the dropped PR's refs arrive; so do a
  row's Open button and a pane switch pressed then (the pick and the press go
  in without the pump between them). With `state/default-repo`
  naming `acme/upstream` (`gh repo set-default` elsewhere) the issue row
  stays on Home with a toast naming that repository; the clone's Issues mode
  is then loaded as acme/upstream (a queued list read), and once `gh`
  resolves the clone to acme/local the issue row opens it in Issues mode with
  #4 selected and the list and Reply re-addressed to acme/local
  (`issue_write_repo_for_e2e`); a reply drafted to acme/upstream's #4 before
  that is not in acme/local's #4 Reply and stays stored for acme/upstream
  (drafts are keyed by the repository written to; `drafts_test` covers the
  hand-over of pre-#940 drafts). For Tier B use a real `gh`
  login, click each switch cell and one PR / issue row of a local clone.
- Home search over incomplete or stale lists (`KAGI_GUI_E2E_ONLY=home_search_incomplete`,
  `tests/recovery/home_search_incomplete.rs`, #1070 / ADR-0219 decisions 2 and 8;
  the filter selects three scenarios: `home_search_incomplete_repos`,
  `home_search_incomplete_refreshing`, `home_search_incomplete_work`): a
  strict offline `gh` answers only the command lines Home's producers build
  (`repo_list_args`, `org_list_args`, `search_args`, `config get user -h
  github.com`) and, with `[]`, Branch Cleanup's `pr list --state merged …`
  read of the PR-less fixture; every other line exits 1. The stub's call
  counts prove each Refresh read again.
  - `home_search_incomplete_repos`: with the organizations unlisted
    (`home-github-orgs-failed`), the user's list cut at `REPO_LIST_LIMIT`
    (`home-github-truncated`) and an organization refusing
    (`home-github-owner-failed`), a search matching nothing keeps those rows
    and draws no `home-list-no-match`; a matching row (`home-gh-acme/needle`)
    stays beside them. Then, over a complete list, the next own-list read
    fails: the list read before stays with its matching row, and
    `home-github-refresh-failed` (the existing `HomeGithubRefreshFailed`
    text, measured directly, not through `note_probe`) is drawn instead of
    no match — also after the search text changes and after a round trip
    through the PRs pane. The next accepted read clears it, and the same
    search is then a plain no match. (A `gh` account change also clears it,
    in `show_saved_list`; this scenario does not exercise that.)
  - `home_search_incomplete_refreshing` and the last step of
    `home_search_incomplete_work`: while a Refresh is still reading
    (Repositories `refreshing`, PRs / Issues `work.reading`), the list on
    screen keeps its matching rows (`home-gh-acme/needle`,
    `home-work-acme/local-4`) beside `home-github-updating` and draws no
    `home-list-no-match`; once the read lands, the complete search says no
    match again.
  - `home_search_incomplete_work`: a failed review / assignee search keeps
    `home-work-failed` and the matching row read before; a list cut at
    `WORK_LIST_LIMIT` keeps `home-work-truncated`. Once every list is read
    in full, Refresh keeps the search text and the complete search draws
    `home-list-no-match` — per pane: the PRs, read in full, say no match
    while the Issues are still cut.

  Two ways of refreshing, on purpose. Every Refresh that must *finish* is a
  real click on `home-github-refresh`. The "still reading" frames instead
  call the button's handler, `reload_home_github`, and draw before
  `run_until_parked`: a simulated click parks the executor, which runs the
  `gh` reads to their end, while the test dispatcher only runs them inside
  its pump — so the frame drawn first is deterministically the one with the
  read running. No sleeps and no hold seam. The note names
  (`home-list-no-match`, `home-github-truncated`, `home-github-owner-failed`,
  `home-work-truncated`) come from a `gui-e2e`-only seam (`note_row` /
  `note_probe` in `src/ui/home_github_list.rs`) that names a `Note` row by
  the `Msg` it shows; default builds draw the same `muted` row without the
  wrapper.

  Two barriers, recorded separately. First: `home_search_incomplete_repos`
  and `home_search_incomplete_work` failed before the fix (the
  organizations row and the review-search failure hidden while searching);
  after it, `home_tab,home_github,home_work,keyboard_nav,home_rows,home_list_place`
  plus the two passed 8 of 8 (KEEP_GOING). Review follow-up: the
  own-refresh-failure step, the refreshing scenario and the work reading
  step failed 3 of 3 before the second fix; after it the same six plus the
  three passed 9 of 9. The final source (with the `SharedString` ownership
  correction of the refresh-failure row and `cargo fmt`) passed the same 9
  and all seven final gates again.

  Tier B (done for #1070, default build, two separate launches, no UI test
  seam): a private strict offline `gh` — synthetic account, rows and
  deliberate failures, not a real GitHub outage, SAML enforcement or
  account switch — and the app's own cache. In the first launch, all three
  panes showed failed reads, filtering, matching cached rows, the "Updating"
  read and accepted recovery; Repositories and Issues also showed cut lists.
  Each pane decided "no match" on its own lists (PRs said no match beside a Repositories
  failure and beside cut Issues); a second own-list failure replaced the
  reason's detail on screen (502 → 503). Holding the own-list read there
  ran into the existing 60 s read timeout, so its frame is a timeout with
  the reason kept, not a recovery; the recovery was the explicit Refresh
  after it. The second launch started on the complete list the first had
  saved, held only the own-list read, and showed the saved rows with
  "Updating" and no "no match" for a query typed meanwhile; after the
  release (about 40 s after launch, driver time, not latency) the accepted
  read turned the same query into a plain no match. Both quit with ⌘Q
  (exit 0, no process left). Not exercised anywhere: clearing the reason on
  a `gh` account change (reviewed in source only). To repeat it, drive the
  same states with a stand-in `gh` on `PATH` in an isolated launch (`USER`,
  `KAGI_NO_RESTORE=1`, `KAGI_LOG_DIR`); images go to the PR through
  `gh --attach`, never into the branch.
- Home's display leftovers of #960 (`KAGI_GUI_E2E_ONLY=home_review_avatar_host,clone_card_ticker`,
  `tests/recovery/home_p2.rs`, #968): a review request whose Enterprise host
  is spelt `GHE.example.com` gets its author's avatar from the fetcher's disk
  cache (`avatar_fetch::cache_path_for_url`, no network) and its row finds it
  (`e2e::home_review_avatar_shown`). The running clone's card closed and
  brought back from its row within a second leaves two redraw tickers
  (`e2e::clone_tickers`) until the older one wakes; after 1.6 s on the test
  dispatcher's clock (`advance_clock`) one is left.
- Keyboard paths of Kagi's tab lists (`KAGI_GUI_E2E_ONLY=keyboard_nav`,
  `tests/recovery/keyboard_nav.rs`, #944): a cell is focused through
  `focus_mode_nav_for_e2e` / `focus_home_pane_for_e2e`; Tab / Shift+Tab are
  pressed as keystrokes (gpui-component's Root moves the focus).
  `e2e::set_github_nav(true)` draws the nav's PRs / Issues cells whether or
  not the machine has a `gh`. The workspace-mode nav: Tab reaches its
  selected cell; an arrow only moves (the mode stays) and Enter / Space
  enter the mode; after →→ one Tab or Shift+Tab leaves the list and the
  other comes back in on the selected cell (#960 review); with a notice up,
  Enter on a cell does not confirm it; a pointer click on a cell gives the
  focus back to the root; after → the focus taken away another way than Tab
  (here the root's own focus) leaves the selected cell as the next Tab's
  stop, not the arrowed-to one (#968). Home's switch: ←/→/Home/End select as they move,
  without wrapping, and read nothing (`home_reads_for_e2e`); with Home's
  search focused the arrows do not reach the switch (Enter is not pressed
  into the single-line field: the harness types its "\n"). The repository
  tab strip (#959): with two repositories and Home, one Tab from the window
  lands on the Home tab and the next leaves the strip (a tab's × is not a
  stop); → only moves (`tab_strip_focused_for_e2e`, Home stays in front),
  Enter switches to the repository, End reaches Home and Space brings it
  back; with a repository in front, the Tab that leaves the strip is not the
  + (Enter there leaves Home behind), and closing Home while its cell holds
  the focus gives the focus to the window. For Tier B: Tab
  to each list, check the ring appears only for keyboard focus, and read the
  roles in Accessibility Inspector.
- Rows of Home's list (`KAGI_GUI_E2E_ONLY=home_rows`,
  `tests/recovery/home_rows.rs`, #959): a stand-in `gh` lists 60
  repositories and two pull requests. Tab from the switch reaches the list
  at its first row and one more Tab leaves it; ↓ forty times focuses `r40`
  and scrolls it into view; ↑/↓ stop at the ends; Tab out and Shift+Tab back
  returns to `r40`; a filter that drops the focused row moves the focus to
  the first row left, and one that drops every row to the window; with the
  search field focused ↓ stays the field's; PR rows step the same way; a
  second stand-in `gh` for another account whose list read fails turns the
  list into Loading then Failed, and the focused row's focus goes to the
  window. Rows
  are focused through `focus_home_row_for_e2e` and read through
  `home_row_focused_for_e2e` (keys `repo:<owner>/<name>`,
  `<kind>:<owner>/<name>#<n>`).
- Home row paging (`KAGI_GUI_E2E_ONLY=home_row_paging`,
  `tests/recovery/home_rows.rs`, #980): 60 repository rows, raw Home / End /
  PageUp / PageDown and Cmd+↑/↓ move row focus; the expected step is the
  number of rows intersecting the list viewport minus one (minimum one).
  After each jump, the focused row's bounds fit inside that viewport.
- Commit graph paging (`KAGI_GUI_E2E_ONLY=commit_paging`,
  `tests/recovery/commit_paging.rs`, #980): a 200-commit fixture with root
  focus presses the raw Home / End / PageUp / PageDown and Cmd+↑/↓ keys,
  checks selection (page step = floor(viewport height / uniform row height)
  minus one, minimum one) and checks the selected row against the measured
  `commit-list-viewport`. Wheel-scroll the selected first row out of view;
  Home reveals it without toggling selection. With root focus retained, End
  and Down behind the open commit context menu leave selection unchanged;
  replacing the shared front-layer guard with a menu_overlay-only guard fails
  the End assertion. End on Home, Branch Cleanup, and the Conflict Mode body
  must also leave the covered commit selection unchanged; the conflict leg
  uses `app_conflict::content_fixture` and a real detector pass.
- Context-menu keyboard (`KAGI_GUI_E2E_ONLY=context_menu_keys`,
  `tests/recovery/context_menu_keys.rs`, #985): with the HEAD commit selected
  and root focus, Shift+F10 opens `commit_menu` at the bottom-left the
  selected row recorded (`context_anchor_for_e2e`), the first enabled item
  focused. End lands on the last enabled item (Reset, drawn as
  `commit-menu-item-5-0`, is always disabled), ↓ wraps past it to the first,
  ↑ back, Home to the first; ↓ to Copy SHA and Enter closes the menu with the
  HEAD SHA on the clipboard and root focused; a second Shift+F10 then Escape
  closes it with root focused. Mutations that fail it: ↓ not skipping
  disabled items, focus not given back on close, the anchor ignored, keyboard
  clicks ignored on items. The focus highlight is Tier B (focus_visible).
  `context_menu_keys_sidebar` (same file): a focused LOCAL `branch:feature`
  row records its bottom-left (within 2px of the measured
  `sidebar-local-feature` bottom); Shift+F10 opens `branch_menu` for
  `feature` there with the first enabled item focused, and Escape closes it
  with the row focused again. Mutations that fail it: the row's action doing
  nothing, focus returned to the window instead of where it was.
  `context_menu_keys_home` (#991 review): a commit menu opened with
  Shift+F10, then `open_home_tab` / `close_home_tab` (⌘T / ⌘W's methods; Tier
  A sends no platform-menu keystroke): the menu is closed right after leaving
  the tab — checked before parking, since the tab's re-read on return also
  drops menus once it lands — and after the selection settles ↓ moves the
  Graph to row 1. Mutation: the departing tab not closing its menus fails
  "leaving the tab closed its menu". `context_menu_keys_short`: a 420px
  window, where the HEAD commit's menu (~535px) is taller than the window;
  End focuses the last enabled item (`commit-menu-item-4-2`) and scrolls it
  inside the window (its measured bottom ≤ 420). Mutation: no scroll on a
  key move fails with the item at 453. `context_menu_keys_reload`: the
  `feature` branch menu opened from its sidebar row with Shift+F10, then an
  external `git checkout feature` and `reload`: the landed reload closes the
  menu (`apply_reload_data` → `close_context_menus`, branch and tag menus
  included) and, with nothing refocusing, the focus is back on the
  `branch:feature` row and ↓ moves on to another LOCAL row. Mutations: the
  pre-fix clears (commit / stash / worktree only) fail "the reload closed the
  menu"; the reload using the tab-departure reset (`reset_context_menus`, no
  focus return) fails "the focus went back to the row". `context_menu_keys_tab`:
  Tab and Shift+Tab inside a commit menu close it and the window has the
  focus (dropping the `MenuDismiss` bindings fails "tab closed the menu").
  `context_menu_keys_covered`: an AppNotice set over an open menu closes it
  on the next frame (`workspace_menus_covered`) and the focus leaves the
  hidden item for the window; so does an Info `MenuOverlay` (About / Keyboard
  Shortcuts take no focus) — only Settings and the command palette, which
  take the focus, are exempt. Mutations: disabling the check fails "the
  notice closed the menu"; exempting every `MenuOverlay` fails "the Info
  panel closed the menu". `context_menu_keys_a11y`: the disabled Reset item
  is drawn with its reason as `aria_description`
  (`menu_overlay::recorded_item_description`), an enabled one with none
  (drawing disabled items without it fails). `context_menu_keys_covered_row`:
  an Info overlay over a menu opened from the sidebar `branch:feature` row
  closes it and the window — not the row — gets the focus (returning to the
  row when covered fails). `context_menu_keys_disabled_live`: with Checkout
  focused, `planning` set (any latched operation) disables it while the menu
  stays open; the focus stays on it and ↓ still moves onto an enabled item
  (tracking focus on enabled items only fails: ↓ never reaches the menu).
  `context_menu_keys_item_appears`: a keyboard slot is an item's place in
  the menu's own list, hidden items included. On the `feature` branch (no
  upstream) the focus is moved to Copy head SHA (`branch-menu-item-4-3`, via
  `menu_overlay::recorded_slot_control`); setting `planning` makes the hidden
  Push appear above it, and the focus stays on Copy head SHA, whose Enter
  copies the SHA. Numbering slots by drawn items only fails with the focus on
  `branch-menu-item-4-2` (Copy branch name).
  `context_menu_keys_row_behind_info` (#1000): with an Info `MenuOverlay` up
  and the sidebar `branch:feature` row (on HEAD~1) focused, Shift+F10 opens
  no branch menu and the Graph selection stays on row 0 (dropping the
  `front_layer == None` guard in `open_sidebar_row_menu` fails: the selection
  jumps behind the panel). `context_menu_keys_last_tab` (#1000): with a
  commit-menu item focused, `CloseTab` on the last tab leaves no menu and the
  root focused on Welcome (dropping the window-slot reset / root focus in the
  `TabClose::Welcome` branch fails).
- Toolbar unavailable reasons (`KAGI_GUI_E2E_ONLY=toolbar_keyboard_reasons`,
  `tests/recovery/toolbar_keyboard.rs`, #972): starting at the root, GPUI's
  `focus_next` visits the rendered toolbar in visual order; an F19 key-down
  identifies the actual focus owner. Pull / Push / Stash / Pop / Undo / Redo
  stay AX-disabled but reachable; Enter and Space produce their localized
  pointer-click footer reason, and the exact value passed to
  `aria_description` is recorded by `e2e::toolbar_description`. The fixture
  fingerprint must remain unchanged. Before adding focus stops this scenario
  failed (none reachable); mutating Pull's description input to `None`
  fails its AX assertion. TestDispatcher cannot prove OS Tab/VoiceOver
  speech (the native AX tree did not expose GPUI content during the #972
  foreground probe). Tier B on macOS with a clean one-commit repository and
  explicit foreground permission: pointer-click unavailable Pull, Tab to
  unavailable Push shows the focus-visible ring, and Enter changes the footer
  to the existing no-remote reason without changing HEAD, status or stash.
  The native `AXUIElement` query returned only the window's title-bar
  controls (`AXGroup` for content had zero children), so actual VoiceOver
  announcement of `aria_description` remains unobserved; do not present the
  Tier A attribute oracle as a spoken-word measurement.
- modal-slot arbitration (`KAGI_GUI_E2E_ONLY=push_failure_keeps_modal,merge_plan_latch,delete_branch_plan_latch,remote_browse_modal_routing`): a push failure lands behind Remote Browse without losing its input and reaches the Failed footer, an Error toast and one durable receipt — no dismiss-only AppNotice, queued or shown after Remote Browse closes (the #747 contract; #824 bisected the stale notice expectation to `e5644c6f`). Delayed Merge/Delete Branch plans wait behind Remote Browse without losing its input, stale plan state, latches, footers, or notices; a reopened Remote Browse rejects an older in-place completion by generation;
- unmerged branch deletion with two confirmations, retained tips, and one-stage merged deletion.
- toolbar centre actions (Pull…Terminal) drawn only in Graph, not PRs/Editor/Analyze
  (`KAGI_GUI_E2E_ONLY=workspace_mode_toolbar`, via the `tb-repo-actions` control bound);
  the same scenario covers the Issues Composer's real New/Reply input-change
  subscriptions, the Reply's lack of a New Issue-only title entity, Markdown
  Preview, scoped Focus Editor, multiline Paste and Undo, and successful write
  settlement consuming its draft without reopening Issues after Graph is selected,
  all without a live GitHub write (ADR-0201; `src/ui/issues_composer_e2e.rs` seeds
  read-side state and enters the production settlement path); since #750 it also
  covers the PR page's shared timeline feed — a seeded line comment's diff hunk
  (`pr-convo-hunk-<n>`) and ```suggestion marker (`pr-convo-suggestion-<n>`) drawn
  inside the shared row — and the pinned PR composer's real toggle click
  (`pr-composer-mode-toggle` → `pr-composer-preview`) keeping the typed text
  across a preview round trip; since #751 it also proves the conversation
  Markdown is *laid out*, not merely parsed: the canonical body
  `tests/support/issues_markdown.md` is cut along its own `## ` headings, each
  section is seeded into the production Thread (`issue-thread-body-4-md`) and
  the production Composer Preview (`issue-composer-preview-md-0`), and the
  drawn box is then selected with a real pointer drag (down / move / up) and
  copied with ⌘C — the clipboard is poisoned with `NOTHING_WAS_COPIED` before
  each drag, so a selection that found nothing fails instead of reusing the
  previous copy. The task list, table and fenced code are asserted from what
  the selection returned, because selection resolves against laid-out text
  runs. `gpui-component`'s `BlockNode` is `pub(crate)`, so a Markdown plugin
  cannot wrap the built-in table/list/code renderer to measure it directly,
  and a plugin that claimed those nodes would be a parallel renderer; the drag
  is the layout-derived seam that remains. Images are never fetched on these
  surfaces (ADR-0142 amendment), so no screenshot ever waits on the network;
  The same scenario exercises the shared Issue/PR label and created-sort menus.
  Its PR-state regression holds a Closed response and checks Refreshing, then
  proves Closed/All results cannot replace shared Open evidence, an Open ticker
  update cannot replace the visible Closed collection, and a late response after
  leaving PR mode cannot restore that collection. Pair with
  `KAGI_GUI_E2E_ONLY=github_evidence_` for session restore/background/detach.
- PR bounded cursor paging
  (`KAGI_GUI_E2E_ONLY='pr_pagination,pr_same_number,pr_paging_survives_periodic_tick,pr_periodic_pending_sidebar_and_other_collections,pr_single_page_periodic_scroll_retention'`
  with `KAGI_GUI_E2E_KEEP_GOING=1`, `tests/recovery/pr_pagination.rs`,
  `tests/recovery/pr_pagination_periodic.rs` and
  `tests/recovery/pr_same_number.rs`): run all ten scenarios — `pr_pagination`,
  `pr_pagination_races`, `pr_pagination_closed_tab_retention`,
  `pr_pagination_all_tab_retention`, `pr_pagination_pending_closed_tab_return`,
  `pr_pagination_pending_all_tab_return`, `pr_same_number`,
  `pr_paging_survives_periodic_tick`,
  `pr_periodic_pending_sidebar_and_other_collections`, and
  `pr_single_page_periodic_scroll_retention` (single-page Open list, real wheel to mid-table, 61-s ticker, anchor/offset unchanged). The six original paging
  cases cover first100 → final120 unique fixture rows, real clipped-tail demand, a failed append's
  native retry click, dedupe, stable anchor and context-menu identity. After
  proving the menu survived append, dismiss it through an actual outside click
  before scrolling to row120: its full-window occlusion correctly blocks the
  underlying wheel route. Filtered-zero and client-filtered lists keep an
  explicit continuation; the real sidebar viewport must reach its control.
  Closed/All actual menu choices survive A→B→A with their rows, selected pane
  and real composing text, reject old-visit appends, and do not turn an
  interrupted initial read into authoritative zero.
  `e2e::queue_github_pr_fetch` substitutes only a page-shaped transport future;
  parser/receiver state, owner/generation/visit/cursor admission and UI
  scroll/click handlers are production consumers. Shared evidence stays Open;
  selected Closed/All belongs to the existing strip. Failed refresh revokes
  continuation, while failed append retains rows/cursor for retry. Do not
  replace cleanup's reduced Vec evidence with a PR list page.
  Opening a fixture PR can legitimately trigger the existing owner-bound L1
  refresh from real local-ref fetch callbacks. The sidebar leg therefore uses
  its strict `OfflineGh` script: explicit `repo view --json url` plus the
  frozen-host/repository Open GraphQL `first: 100` request return a real
  pageInfo-bearing first page; unsupported calls fail. Do not suppress a
  production ref-refresh callback to keep a test cursor. Sidebar measurements
  instrument the existing viewport with gui-e2e-only `measure_inside`, not a
  layout-changing default-build wrapper.
  The detail unit consumers (`cargo test -p kagi --lib github_pr_detail`) and
  receiver consumers (`cargo test -p kagi --lib github_pr_state`) cover
  shared/strip/opened same-head payload ownership, successful empty payloads,
  stale heads, lost owners and old answers. Retention must not demand
  off-screen L2: visibility stays selected-state-only, with concurrency two,
  200 ms debounce, 500 ms maximum wait and two-row overscan; L3 remains
  opened-only. Checks filters must retain real Fresh passing payloads without
  treating dehydrated reappended rows as Fresh.
  True table visibility comes from clipped prepaint, not `uniform_list`'s
  layout-only `render_items(0..1)` call. The first actually rendered row's
  non-layout-affecting canvas defers one visible report guarded by active owner,
  full list revision and PR-workspace/home; measurement must not publish demand
  or prune the tail queue. Require page-two tail detail demand after scrolling,
  as well as the unchanged two-row overdraw and maximum two L2 requests.
  The two periodic scenarios start `ensure_startup_repo_io` and its real ticker:
  a strict private gh producer answers real bounded GraphQL/parser and status
  requests, and receipts distinguish first-page reads from detail refreshes.
  `pr_paging_survives_periodic_tick` checks the 59-second negative control,
  advances across the ordinary 60-second tick, and retains all 120 identities,
  generation, actual tail bounds/scroll anchor, typed filter, parked PR owner
  and the live composer entity/value. Change only the producer's server status
  from SUCCESS to FAILURE and assert actual shared/opened/visible-tail `ci` and
  check payloads transition; an unchanged Fresh flag or query count is not
  proof of refresh. Manual Refresh must then reset to the authoritative first
  page, revoke continuation/generation and reject a held old append.
  `pr_periodic_pending_sidebar_and_other_collections` clicks the sidebar's
  actually painted Load more, holds its append across a tick, proves no shared
  L1 replacement while the opened PR's checks change, then accepts 120 rows
  and retains the reachable sidebar tail across another tick. Real Closed and
  All menu choices with pending strip appends must allow ordinary shared Open
  ticker reads without replacing selected strip membership or revoking its
  append; departure to Graph must resume normal first-page replacement.
  Retention admission is only active PR workspace + no explicit Closed/All
  strip + (shared Open pages > 1 or append pending). It preserves last-accepted
  membership, not the newest authoritative list: use manual Refresh to discover
  new/closed/deleted PRs. Owner/manual refresh and background consumers retain
  their ordinary cutover; deferred ticks still refresh visible/opened details.
  `pr_same_number` binds the live composer by `(SessionId, PrKey)`, parks through
  the recorded owner, and proves real `InputState::replace`/drawn drafts stay
  isolated across same-key sessions and close/reopen. The Closed/All producer
  also edits the live input and verifies the parked value before departure;
  writing `PrTab::comment_draft` while the input is bound is not an edit.
  These native fixtures do not prove key/paste delivery, real GitHub default
  paging, FPS or screenshots. Tier B must separately use the default build,
  its genuine first100/pageInfo, actual tail/retry/filter controls and the
  actual server-ordered remainder; compare unique loaded identities and
  `[kagi] github: prs page=N loaded=M has_more=true|false`. Do not infer which
  PR is absent from its number or alter the limit to make the check pass.
  Exhausted-table regression: item count must be
  `rowcount + usize::from(has_more)`, not an unconditional full-height terminal
  slot. With `cursor=None`, the final real row must actually paint inside the
  tiny body at 940×660/167%; loading/retry and pending continuation remain.
  Pair with `pr_dashboard_responsive`'s eight width/locale/zoom cells and
  `pr_list_roles,issues_pagination` without repinning assertions. PM's local
  seven-stage post-repair gate passed; its scoped native selection included
  these three consumers, all six original paging cases and both periodic cases.
  The gate manifest is
  `/tmp/kagi-hig-audit-20261008/pr-paging-terminal-row-final-gates/manifest.json`;
  this is local source acceptance, not hosted CI or merge acceptance.
  Historical Tier B #1107 evidence (before the terminal-slot repair):
  `/tmp/kagi-hig-audit-20261008/pr-periodic-after-native/proof.json` records the
  default app PID 45189/window 4156 and source after the true-prepaint fix and
  nine native PASS scenarios. Genuine first100 → explicit Load more → 120
  reached the actual server-ordered tail PR #28. The actual 65.26914-second
  no-input interval retained 120 membership and exact tail/viewport, with 23 gh
  receipts: zero L1 GraphQL replacements, 22 L2 status queries including #28,
  and one opened #265 body refresh. Viewed before/after photographs showed
  determining/ready aggregates 75/45 → 107/13 with the anchor retained; no manual
  refresh or state switch occurred during the interval. This is evidence of
  automatic retention with volatile refresh, not newest membership, all future
  tick behavior, FPS, latency or CPU improvement. The earlier CPU sample did
  not overlap input. Temporary diagnostic reads were removed after that build;
  this historical evidence does not accept the later terminal-slot repair.
  Fresh narrow default-build evidence is separately recorded in
  `/tmp/kagi-hig-audit-20261008/pr-paging-terminal-tierb/narrow/observed.json`,
  with frozen-source SHA-256s and exact owned PID 61715/window 6182. Logical
  940×660, zoom 1667, Apple Light/English window-only captures are 1880×1320
  (ratio 2 from capture/bounds, not a backingScaleFactor query). A real visible
  Load more accepted 100 → 120, `has_more=false`; eight wheel events reached
  actual last PR #28 fully visible in the tiny body with no empty terminal slot.
  The fresh periodic photograph retained 120 and #28, while readiness changed
  5 → 6; the observed timeline had no new list GraphQL after extension.
  The recorded 176.033609 seconds is page-request-to-capture, not exact
  response-acceptance-to-capture. An initial tick before append added a third
  list GraphQL call, so do not report two total list calls for this run.
  Real #28 opening showed commits1/files1 and conversation counts0/0/0, but its
  photograph is swimlane-only, not acceptance of the narrow Overview body.
  Cmd-Q/original Popen exited 0, exact executable PID path and owned apps were
  absent afterward. This establishes narrow startup/terminal paint only:
  wide-window, dynamic resize, Copy/Paste/host clipboard, CPU, FPS and latency
  were not accepted by these observations; driver delivery time is not FPS.
  Fresh wide default-build evidence:
  `/tmp/kagi-hig-audit-20261008/pr-paging-terminal-tierb/wide/observed.json`
  records unchanged frozen-source hashes, exact owned PID 69326/window 6200,
  logical 1392×883/100%, Apple Light/English and window-only captures, ratio 2.
  Eight table wheels caused natural clipped-prepaint continuation (no explicit
  Load more), 100 → 120; four further wheels reached last PR #28. A settled-tail
  photograph and the periodic photograph 155.08867 seconds later retained
  Loaded120 and the same #80..#28 viewport, while Ready15 → 13 and real
  `pr view` receipts at roughly 60-second cadence witnessed ongoing checks.
  No list GraphQL occurred after extension through the periodic photograph,
  but four occurred through extension: initial + two legitimate pre-extension
  ticks + one non-null cursor read. Do not report two total list calls.
  Cursor-request-to-periodic-capture 229.741505 seconds is not exact acceptance
  timing. Actual large PR #12 Overview displayed a 48,002-byte body with real
  transport 24 comments/288,048 body bytes, two reviews/268 body bytes and
  accepted 40 line comments (visible Review66), commits3/files2, additions22,010.
  This does not prove complete body/comment-tail, diff or thread interaction,
  or performance. Settled All PRs returned to the same120/tail28; the immediate
  capture preceded that state and supplies no navigation latency.
  Small #28 Overview body102 bytes was visible, comments/reviews0, commits1/files1.
  Physical unsent ASCII `PM_PR28_DRAFT` stayed out of #12's composer; six sidebar
  wheels reached #28 independently while #12 Overview remained, and clicking
  #28 restored the exact draft. This accepts one-session/two-PrKeys only;
  same-PrKey/two-session default clicks remain dependent on separate overlay
  baseline repair/integration, not proven by this run. No Submit/Copy/Paste,
  host clipboard or foreground action occurred; original Cmd-Q/Popen exit0,
  exact executable PID path absent and owned apps0 establish cleanup.
  The matched same-PR narrow/wide photographs corroborate separate
  [#1118](https://github.com/TomiXRM/kagi/issues/1118) detail geometry: narrow
  #28 Overview body missing, wide present. Do not call this narrow-detail #1095
  acceptance or independent baseline-dev reproduction. Dynamic resize,
  matched CPU/FPS sampling, full diff/thread acceptance and external review/CI
  remain unobserved; no future post-doc gate or merge is implied.
- PR parent render borrowing (#1108 / ADR-0200 §7):
  scope Tier A to
  `KAGI_GUI_E2E_ONLY=pr_same_number,workspace_mode_toolbar,pr_threads_via_gh`
  with `KAGI_GUI_E2E=1`, `--features gui-e2e --test gui_e2e_runner` and an
  isolated log/target directory; PM owns execution when assigned.
  `pr_same_number` uses two private bare Git remotes with genuine PR refs and
  the existing repository-qualified fixture `gh` subprocess/parser. Its held
  list refresh is admitted before switching B#7 → A#7; release must accept
  the same generation and draw B's distinct refreshed title in the real
  dashboard row while retaining A's exact multiline UTF-8 composer text and
  both parked drafts. Input uses the real `InputState::replace`, not keyboard
  typing or IME. Existing body/count/feed geometry, reload and normal unmount
  assertions remain. This is not an authenticated GitHub or allocation-counter
  oracle. Require `workspace_mode_toolbar: PR section exercised`; overall
  PASS with its PR section skipped is insufficient. `pr_threads_via_gh`
  supplies the independent real-ref/production-parser badge witness.
  Do not invent a separate scenario PASS marker or case count from a final
  `PASS filtered scenarios`.
  The subsequent PM final lane passed all seven stages: build, this scoped
  native selection, workspace tests, Clippy, fmt check, invariant checks and
  default build (all exit 0; total wall 429.369851 s, native 23.138066 s).
  Production hashes remained unchanged; the formatting-only final
  `tests/recovery/pr_same_number.rs` SHA-256 is
  `098376339bb864ee7f69fcaf041fc15ef6e4d945157160ebb6e0170d45906f01`.
  This records local gate completion, not public CI/external review or a
  runtime performance result; the Tier B limits below remain.
  Tier B: use the exact default binary, source hashes, one named owned window,
  unique USER, `KAGI_NO_ACTIVATE=1`, `KAGI_NO_RESTORE=1` and a retained private
  log directory. Read actual PR12 / small PR280 producer counts first; Review
  counts raw reviews + issue comments + line comments, not description or
  only nonempty posts. Witness description first/terminal sections, review
  first/middle/final posts, anchored thread open/close, whole-PR versus selected
  commit files, return navigation and exact unsent draft restoration without
  Submit. Ordinary commit element IDs are not measured-control selectors.
  For the 2026-10-10 PM run, accepted evidence is EN / Apple Light / 100% at
  1392×883: PR12 description 0001 → 0140, Review 66 and large-comment
  paragraphs 26–28, a real line-2 thread, the 20005-line file's rows 72–102,
  selected-commit Files 1 / +1980, and PR280's empty conversation / ASCII
  draft return. Final Review tail, large-diff terminal row, split, JA / 167%
  and same-key two-session default smoke remain unaccepted. A -50000 wheel
  returned to the top, so it proves neither the tail nor a product cause.
  No matched input-overlapping before/after sample was accepted: compile/run
  duration, driver duration and source copy removal are not FPS/CPU/latency
  or smoothness evidence. Measure performance separately with the same
  warmed workload and monotonic sample-ready/input overlap. Preserve historical
  compile-contamination and fixture-generation/readiness failures as failed
  attempts, never relabel them as a production performance Before.
- GitHub Markdown preparation reuse (#1091 bounded slice, ADR-0142 amendment 3):
  `KAGI_GUI_E2E_ONLY=markdown_preparation_edges`
  (`tests/recovery/markdown_preparation_edges.rs`, a child of
  `tests/recovery/workspace_mode_toolbar.rs`) keeps one production Issue Thread
  body/window/owner/Markdown ID across frame replacements. It reuses the
  existing measured-body physical drag (down/move/up) and ⌘C helper, poisoning
  only `VisualTestPlatform`'s private clipboard before every copy; do not replace
  that platform with the default real-app clipboard or access the OS clipboard.
  Equal-byte-length different GFM raw bodies must copy exactly the new rendered
  text, excluding the old sentinel. The raw body then stays empty while EN → JA
  → EN must copy the current typed `Msg::IssueNoDescription` placeholder and
  exclude the prior locale. Bounds must fit inside the viewport; settings and
  process-global language are restored, the repository fingerprint is unchanged,
  and the shared `unmount` closes the window. This is painted-text consumer
  evidence, not a cache-counter, source-text, mock-copy or nonempty assertion.
  The preparation payload is existing window-keyed state under the body ID in a
  separate namespace: one raw snapshot + format + prepared `SharedString`, replaced
  on exact raw-byte or format changes. Current style, literal font features and
  the existing privacy preparation path are supplied to the same renderer.
  At the 2026-10-09 checkpoint, `conversation_markdown_test` passed 10 semantic
  tests; one native run separately passed the four existing consumers
  `pr_same_number`, `pr_threads`, `pr_threads_via_gh`, `workspace_mode_toolbar`;
  a later native run passed `markdown_preparation_edges` alone (4 PASS and
  1 PASS from separate runs, not one five-scenario run). The default build also
  completed. The default app's real warm PR #12 After sample had 6,021 main-thread
  stack observations and none for `prepare_github_markdown`, `images_as_links`,
  `flatten_html_blocks`, `pad_inline_code`, `sanitize_github_markdown` or `to_mdast`;
  baseline had 549 preparation observations
  under 5,893 main-thread observations. Corpus and Apple Light/EN/100% v2/1392×883
  logical viewport matched; baseline `adf3ade12d9786b198d4821f30dd4f629c12cca4`
  differs from implementation base `94cf1849dc7af265995a58ee1d2c2c546aa7fc06`
  by a Pull-label change outside this UI surface. All eight requested −80 wheel
  events landed within each eight-second sample; After PR #12 delivery completed
  1.709031 seconds after readiness. Screenshots show section 001 → 49/50/51,
  not full-conversation traversal. The short real PR #280 control showed its
  complete one-line body, no reviews, one commit and one file (+2), with eight
  events delivered in 1.733571 seconds at the same settings. The owned original
  app exited normally through ⌘Q (exit 0, process gone).
  Zero observations in this one After sample are not proof of zero calls.
  Raw sample observations/input delivery do not establish CPU%, FPS, latency,
  speedup or actual scroll distance. At that preparation-only checkpoint, cold
  huge-post preparation/layout, PR clones and Issue conversation ownership/
  virtualization/offscreen Copy still required separate implementation and proof.
  The subsequent Issue Thread cutover is documented immediately below and in
  ADR-0198; do not reuse the preparation-only gate as its latest-source acceptance.
  The later final required gate passed build, native, workspace tests, Clippy,
  format check, invariants and default build, all exit 0. Its single native
  run passed the four consumers above plus `markdown_preparation_edges`,
  five of five. Whole gate wall was 406.020904 s; workspace process wall was
  351.394085 s, including compilation/process overhead, not isolated test-body
  timing or speedup. This does not complete the full issue or imply hosted
  CI, exact-head review or dev merge. Publish as `Refs #1091`, not `Closes`.
- Issue Thread managed virtual conversation (#1091, ADR-0198 amendment):
  `KAGI_GUI_E2E_ONLY=issue_conversation_` selects all **eleven registered** native
  scenarios in `issue_conversation_selection.rs`, `issue_conversation_lifecycle.rs`,
  `issue_enabled_input_copy.rs` and `issue_offscreen_select_all.rs`.
  Keep the runner's offscreen AppKit windows,
  `VisualTestPlatform` private clipboard, per-scenario unmount and fixture
  isolation; never use the OS clipboard, suppress a native hook panic, change
  original assertions/coordinates/producers, or substitute a mock selection.
  With source frozen and execution reserved to PM, the bounded native recipe is:

  ```bash
  KAGI_LOG_DIR="$(mktemp -d)" KAGI_GUI_E2E=1 \
    KAGI_GUI_E2E_ONLY=issue_conversation_ KAGI_GUI_E2E_KEEP_GOING=1 \
    KAGI_GUI_E2E_TIMEOUT_SECS=90 CARGO_TARGET_DIR="$PWD/target" \
    cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
  ```

  The consumer oracles are:
  - `offscreen_copy`: exact forward/reverse A→Z including unpainted middle,
    with mousemove→Copy in the same update before paint.
  - `giant_copy`: all 180 rendered paragraphs through the actual last
    continuation and visible end Reply composer, not a truncated preview.
  - `accepted_identity`: actual opaque-ID true B survives equal-author/time
    reorder and prepend; same-length source mutation/deletion poison old Copy,
    while a fresh physical drag copies the new/surviving post.
  - `refresh_anchor`: unchanged refresh, prepend and earlier removal retain
    the true post/intra-post pixel anchor; a real producer failure preserves
    accepted selection/anchor and draws error chrome.
  - `geometry`/`edge_drag`: width/zoom/theme reflow preserves unchanged source
    endpoints, fresh drags use current geometry, stationary edge ticks reach
    never-painted posts, and mouse-up/departure cancels scrolling.
  - `delayed_rejected`/`two_owners`/`copy_priority`: held refresh keeps accepted
    content; superseded generations, repository retarget, session switch/drop,
    modal displacement, mode departure and home return cannot leak Copy before
    paint or revive an old range on reentry.
  - `enabled_input_copy`: actual production **enabled** Reply SelectAll/Copy
    returns exact `draft🙂`; physical Backspace empties it, empty Copy leaves
    poison, and physical Tab then Copy returns the **original** conversation
    selection without reselect/refocus/reload/activation. This recovery oracle
    rules out “empty Copy passed because conversation was already retired.”
    Departure stops Copy; draft, HEAD, staged paths/OIDs/modes, working bytes,
    refs, stash and oplog invariants are checked.
  - `offscreen_select_all`: physical first-paragraph drag copies `PARTIAL`;
    bounded wheel events remove that focused post's control witness with another
    post still visible, and offscreen Copy still returns `PARTIAL`. Physical
    Cmd-A/C must expand to exactly `PARTIAL\n日本語🙂 café code🧭`, the full
    focused post, not raw GFM or the conversation. No direct SelectAll dispatch,
    focus/reselection after unmount or model injection. Complete draft/repository
    invariants and normal cleanup precede the final byte oracle. Strong/emphasis
    fixture avoids incidental existing code-span padding, without changing it.

  The managed ancestor now delegates SelectAll to the actual focused member's
  existing state handler. Mounted/Input consumers retain priority; current
  focus/lease/scope/weak-member/source checks fail closed. No focus steal,
  whole-conversation SelectAll, new global binding/cache/scan or retired geometry
  revival. SDK disabled Input coverage is not enabled native proof.

  SDK coverage is separate. From the matching SDK checkout with its own target,
  PM runs `cargo test -p gpui-component --lib text::` for logical, canonical
  parser and legacy `window_selection`/`text_view` paths, then
  `cargo test -p gpui-component --lib` for the complete lib suite. Its manifest
  has `doctest = false`; a lib/package pass is not documentation-test evidence.
  **No `--skip`, ignored case, caught panic or fake native handle.** The SDK's
  `disabled_input_copy_does_not_fall_back_to_conversation` honestly uses
  `Input::disabled(true)` and retains exact draft/empty clipboard assertions.
  It is disabled/read-only boundary coverage, not enabled Input proof:
  GPUI TestWindow lacks the native input-handler hooks. Enabled editing/focus
  belongs to the real AppKit scenario above, not a headless workaround.
  The focused `stable_selected_post_moves_without_transferring_native_authority_or_old_lease`
  specifies same-group stable interval preservation, fresh lease, retired
  geometry/viewport/edge task, old-lease isolation and selected-source failure.
  `RenderedDocument` is the single rendered UTF-8 extraction path; obsolete
  `ParsedDocument::text`/`BlockNode::text` wrappers are removed without removing
  real paragraph/parser consumers or canonical plugin-text assertions.

  Actual PM checkpoint: `bg_600` (raw artifact 2153) was **9 PASS / 1 FAIL of
  10**, including the complete enabled Input recovery oracle. Only true-B
  equal-metadata reorder failed. After the stable-interval SDK setter fix,
  `bg_605` ran `accepted_identity` alone: **1 PASS / 1**, 37.67 s process wall
  including build/runner overhead, with original native assertions/coordinates/
  producers unchanged. Do not combine those separate runs into an all-ten PASS.
  The subsequent frozen-source `bg_607` (raw artifact 2173) ran the complete
  SDK lib suite: **334 PASS / 0 FAIL, 0 ignored, 0 filtered**, without skips;
  test bodies 0.06 s, process 12.75 s. It included logical/legacy/parser/custom
  plugin consumers after extraction cleanup and disabled fixture migration.
  PM inspected the compile output: both obsolete text warnings were absent,
  with existing dependency future-compatibility warnings remaining.
  `bg_608` (raw artifact 2175) then ran **all ten together: 10 PASS / 0 FAIL,
  exit 0**, process 64.80 s including 28.35 s compilation. Original nine
  assertions/coordinates/producers and the full enabled counterpart remained.
  These passes precede the authorized formatting-only phase, not an execution
  of a subsequently published SDK revision. They supersede the older skipped
  SDK run's incomplete coverage, not unrelated gates.
  SDK workspace fmt check `bg_609` (raw artifact 2177) failed, exit 1/2.63 s,
  with untouched assets/macros/stories baseline and changed-code diffs.
  Config explicitly selects Rust/style edition 2024. Format only changed Rust
  scope with `skip_children=true`; do not hide the baseline failure by
  blanket formatting or a 2021 override, or claim full-repository format green.
  The authorized 13-target scoped rustfmt and same-target `--check` then both
  exited 0 (0.15/0.16 s) using the unchanged config and `skip_children=true`:
  `root.rs`; text's `document.rs`, `format/markdown.rs`, `inline.rs`,
  `inline_flow.rs`, `mod.rs`, `node.rs`, `state.rs`, `window_selection.rs`,
  `logical_selection.rs`, its two test files and `rendered.rs`.
  `text_view.rs`'s changed Copy line already matched formatting; its unchanged
  line-463 trailing whitespace was deliberately preserved. No semantic split
  or Root refactor was introduced. Scope success is not workspace fmt success.
  PM's later post-format `bg_615` (raw artifact 2195) reran full SDK lib:
  **334 PASS / 0 FAIL**, bodies 0.07 s, followed by UI-lib Clippy exit 0;
  combined process wall 42.52 s. Both obsolete text warnings remained absent.
  One new Clippy `too_many_arguments` diagnostic at the private nine-field
  `register_logical_inline` paint boundary was resolved afterward by a
  localized documented allowance: source identity/range and native
  layout/lines/hitbox stay independent; Geometry's owner/scope/weak presence
  is constructed only after registration guards. No API/geometry/allocation
  change or extra argument-bundling payload was introduced.
  Exact-annotation UI-lib Clippy then passed in PM `bg_619`, exit 0,
  process4.19 s including check/compile3.99 s, against logical-selection SHA-256
  `f1b028c23b42d14cafced1e8ca75d1819a6d47dca5ea80fdf24a9ef829a2b8ab`.
  No new argument-count/dead-code warning remained; only pre-existing
  dependency future-compatibility notices (`block`, `proc-macro-error2`).
  PM `bg_620` (raw artifact 2207) then passed the exact-source complete SDK lib
  suite, **334 PASS / 0 FAIL, 0 ignored, 0 filtered**, bodies 0.07 s, and the same
  13-target configured-2024 `rustfmt --check` with `skip_children=true`, exit 0;
  combined process wall7.33 s.
  That SDK-run checkpoint alone did not establish a public revision or
  post-format native rerun, and scoped fmt is not whole-workspace fmt green.

  PM subsequently published the original-b004-compatible fix on the existing
  [TomiXRM/gpui-kit fork](https://github.com/TomiXRM/gpui-kit), normal branch
  `fix/kagi-logical-selection-b004-20261010`, commit
  [`941f20e6c374ea80d2bab9cd08fc96021280db13`](https://github.com/TomiXRM/gpui-kit/commit/941f20e6c374ea80d2bab9cd08fc96021280db13).
  `bg_627` push exited0; PM's exact-head `ls-remote` matched. At that first
  cutover both temporary UI/assets paths were replaced with the same Git/full
  `941f20e6` revision, without latest-family upgrade, macros patch, alias or
  path shim. This is historical, not the final public revision below.
  SDK code at publication retains exact-source lib334/Clippy/scoped13 evidence.

  PM then resolved the actual consumer lockfile: targeted `cargo update`
  `bg_632` first failed to match the path-to-Git package specification
  (0.09 s, no compile); `cargo metadata --format-version 1` subsequently
  succeeded, 6.523 s (artifact 2230). Only UI0.5.2/assets0.5.1/macros0.5.1
  were added at the exact public commit, each name selected once in metadata.
  GPUI0.2.2/platform0.1.0 retained Zed
  `90b3aa0b3bd3b453775b11a386907c7ac9acd997`; no duplicate macros source,
  latest-family upgrade, lock hand edit or path shim. Those initial resolution
  receipts are not final-ae37 gate acceptance.

  A review-discovered offscreen focused-member SelectAll regression has fresh
  paired evidence on the same corrected GFM/emphasis/Unicode fixture:
  public941 `bg_650` **0 PASS/1 FAIL**, left `PARTIAL`, right unchanged full
  rendered literal, process17.40 s/compile8.97 s; local integration `bg_651`
  **all11 together 11 PASS/0 FAIL**, process52.63 s/compile8.48 s (artifact2322).
  Original-ten oracles/events/coordinates/producers remain unchanged. Earlier
  inline-code fixtures `bg_647`/`bg_648` remain separate historical failures.
  Do not strip U+2009, repin an observed After string or combine changed oracles.

  Native651 preceded the final SDK three-file formatting pass. PM configured
  edition/style edition2024 + `skip_children=true` format/check exited0/0.27 s.
  Post-format SDK `bg_653`: lib334 PASS/0 failed/ignored/filtered, body0.06 s/
  process11.19 s; UI-lib Clippy0/process4.11 s, combined15.40 s, no new warnings,
  only existing `block`/`proc-macro-error2` future-compatibility notices.
  Whole SDK baseline fmt FAIL was not rerun/declared green; doctests disabled.

  Final public SDK is
  [`ae37bfd433781abe44e15edd40867fac1b7b3b2c`](https://github.com/TomiXRM/gpui-kit/commit/ae37bfd433781abe44e15edd40867fac1b7b3b2c):
  PM `bg_657` explicitly pushed GitHub normally/FF941→ae37 and exact public
  head matched. `bg_656` first mistakenly pushed clone-origin/local Cargo-cache
  branch;657 conditionally deleted only that mistaken new ref at the exact
  expected revision, without deleting checkout/files/objects. Do not claim the
  cache was entirely untouched or internal review equals external approval.
  Both final UI/assets patches are the same Git/full ae37 rev, no TEMP paths/
  comments. Actual metadata exited0/1.526879 s: UI0.5.2/assets0.5.1/macros0.5.1
  each once on publicae37; original GPUI0.2.2/platform0.1.0/Zed90b3aa0 unchanged.

  Final Root `bg_658` after PM format passed **all7**, total557.496848 s:
  build25.170750/native52.765071/workspace458.066894/Clippy15.626914/
  fmt2.794284/UV2.494605/default-build0.575039. Receipts:
  `issue-select-all-public-ae37-final-gates/manifest.json` and stage raw logs.
  Public native argv was `cargo test --timings -p kagi --features gui-e2e
  --test gui_e2e_runner -- --nocapture`, under `/usr/bin/time -l`, overrides
  `KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY=issue_conversation_`.
  It reports **PASS filtered scenarios**, not an explicit KEEP_GOING11 count;
  local651's explicit11/11 remains separate. Workspace uses the real target
  runner on the same monotonic epoch. These are process walls, not UI latency
  or causal performance comparisons.

  Final public default Tier B:
  `issue-select-all-public-ae37-tierb/observed-public-ae37.json`, original owned
  Popen42755/exactexe/largest layer0 WID6126. Explicit `USER`,
  `KAGI_NO_ACTIVATE=1`, `KAGI_NO_RESTORE=1`, `KAGI_LOG_DIR` isolation;
  capture2784×1766/logical1392×883 gives ratio2, not a queried NS backing scale.
  Real Issue281 has two80-paragraph GFM/Japanese/emoji/café posts. Mounted
  physical Cmd-A selected body, ten−120 wheel events reached distinct comment
  071–080 and Reply with body outside, offscreen Cmd-A did not select the other
  post, ten+120 events returned original body selection without re-click.
  It began fully selected: partial→full exact bytes are native-private-clipboard
  proof, not default screenshots. Actual unmount is the native control witness.
  No default Copy/Paste/host clipboard. Zoom100→110→100 and AppleLight→
  CatppuccinMocha form/content observed; Settings changes focus and prior text
  selection was not shown on return, not modal selection-retention proof.
  No new width/perf/FPS/CPU/matched timing. Historical941 photos/8s samples
  remain historical. Cmd-Q0/Popen-wait0/proc_pidpath0/persistent owned0 cleanup.

  No private SDK absolute patch path is a final install recipe. Native event
  simulation is not hardware/IME proof. Broader#1091 performance/width/
  comparison and PR-conversation scope, hosted CI, independent exact-head review,
  Kagi PR publication/merge remain pending. Keep `Refs #1091`, not `Closes`.
- Issues cursor pagination (`KAGI_GUI_E2E_ONLY=issues_pagination`,
  `tests/recovery/issues_pagination.rs`): the production virtual viewport loads
  100 → 200 → final-page rows without resetting the scroll anchor; an offline
  response retains rows/cursor and the measured retry control resumes the same
  page. Refresh returns to the first page. Only the transport future is queued
  through `src/ui/issues_composer_e2e.rs`; owner/generation settlement and actual
  scroll/click handlers remain production code. Pair with `workspace_mode_toolbar`
  for Composer, sidebar filters, thread return and focus regressions. For a live
  read-only Tier B check, use a repository with more than 100 open Issues, scroll
  the main viewport to its tail, and compare the loaded badge with
  `[kagi] github: issues page=N loaded=M has_more=true|false`. A successful manual
  refresh starts again at page 1.
  A nonempty Mentioning subset must not auto-page at its tail; its measured
  `issue-filter-load-more` click continues exactly once. Returning to Recent
  restores automatic continuation. Native clicks scroll the sidebar first if
  an expanded collection puts the next tab header outside its viewport.
  #791 extends the final page to 300 loaded rows. A real wheel event must move
  `logical_scroll_top().item_ix` while the drawn frame adds zero Issue-view
  recomputations; typing into the real filter input must add exactly one across
  main and sidebar. The gui-e2e-only counter is adjacent to the production
  `apply_issues` call. Tier B uses a repository with 300+ open Issues: after
  two additional pages settle, wheel within the loaded rows and require no new
  `[kagi] issues: view recomputed loaded=N visible=M` lines. A filter change is
  the positive control. Keep paging/refresh/login changes outside that scroll
  interval, since they legitimately invalidate the derived view.
- Issue write failure ownership
  (`KAGI_GUI_E2E_ONLY=issue_failure_notice_survives_tab_switch`,
  `tests/recovery/issue_write_owner.rs`): a recorded Create failure that lands after
  leaving its owner remains in Operation Log without replacing or extending the
  current tab's modal queue, without invoking a GitHub transport;
- New Issue labels / assignees (`KAGI_GUI_E2E_ONLY=issue_create_fields`,
  `tests/recovery/issue_create_fields.rs`, #866): the composer's measured
  `issue-field-open-labels` / `issue-field-open-assignees` (gpui-component
  `Button`s) open the shared field picker (`FieldTarget::NewIssue`), by click
  and by keyboard: `keyboard_open` steps the window tab order from the title
  and sends Enter / Space as key down + key up (GPUI clicks on the up; a bare
  `simulate_keystrokes` never activates a focused element). Apply only stores
  the picks. An offline `gh`
  serves labels from a file the scenario edits, so a label deleted between the
  pick and the measured `issue-composer-submit` is refused before
  `gh issue create` runs (Refused oplog entry, toast naming the label, body and
  picks kept); the retry sends `--label`/`--assignee` and empties the picks;
  the receipt carries `issue_fields` (Operation Log `labels:` / `assignee:`),
  a plan-time refusal (body that is only a fence → no title) included.
  Assignees are picked keyboard-only: Space opens the picker, tab order from
  the filter box reaches the row Buttons (Space toggles, `aria_selected`) and
  Apply (Space). Apply saves the picks with the draft, and a composer dropped
  with `forget_issue_composer_for_e2e` reloads text and picks through the
  production load (#903; file format and the `.corrupt` set-aside are pinned
  by `crates/kagi-git/tests/drafts_test.rs`).
  `issue-composer-posted-as` and the avatar marker
  `issue-composer-viewer-<login>` follow the login on the repository's host
  (`github_host_logins`, read with `gh api user [--hostname]` after an Issues
  read; `?` until then) — never the window-global `github_login`. Pair
  with `pr_fields_escape_focus` and `workspace_mode_toolbar` for the PR picker;
- Field picker ownership (`KAGI_GUI_E2E_ONLY=field_picker_owner`,
  `tests/recovery/field_picker_owner.rs`, #904 review): a New Issue or PR
  picker opened in tab A and applied after `switch_repo` to B (which shows
  the same PR number) is dropped by the switch — `PrFields` is repo-scoped —
  so neither composer changes and no `pr-edit` is sent. The picker also
  carries its `owner`; Apply only touches that session;
- Risk-based Pull / Push (#1690): run
  `KAGI_GUI_E2E_ONLY=pull_safe_dirty_intent,pull_safe_merge_intent,pull_history_conflict_choice,push_safe_intent,pull_quiet_late_dirty_refuses,push_non_ff_refuses,pull_auto_stash_`.
  Safe current-branch Pull and Push execute from the user's command without a
  second dialog. Compare unrelated staged path/OID/mode, split unstaged edits,
  untracked bytes and stash entries, not just a clean status or new HEAD.
  Actual overlap offers Review changes / Stash & Pull; committed-history
  conflicts route to Merge review, never to stash as a cure. A late dirty-set
  change refuses rather than silently stashing.
  `pull_auto_stash_overlap_preview` starts with real toolbar focus and Return
  keydown/keyup in EN/JA. Hold the admitted Pull, assert the rendered confirm
  controls are gone while busy, then repeat Return and require no second Pull
  or fetch. Keep failure/restoration details durable without a dismiss-only
  error modal.
  Tier B uses disposable clones and their own bare remote. Photograph quiet
  dirty Pull and quiet Push completion; for overlap, set a fixture-only delayed
  upload-pack after the choice is visible and capture Return → card gone while
  Pull is still running. Restore the fixture config afterward. In Japanese,
  inspect committed-conflict choice → Merge review → Cancel and verify unchanged
  HEAD, index and working files. Never use the user's checkout for these writes.
- Consistent local sync entry admission (#1697): run
  `KAGI_GUI_E2E_ONLY='entry_consistency_,pull_freshness_clean_updates,push_safe_intent,pull_safe_dirty_intent,pull_current_'`
  with KEEP_GOING. Exercise actual toolbar, sidebar menu, command palette and
  configured shortcut routes, not a direct call to the shared admission helper.
  Safe Push/publish must admit exactly one recorded operation without another
  card and without changing staged content or working files. For non-current
  clean FF Pull, hold the admitted write after its owned fetch: progress names
  the target branch, no card remains, and release advances only that branch.
  Require the target in the result footer and a successful persisted receipt;
  checked-out HEAD, staged content and working files remain unchanged.
  `entry_consistency_sidebar_publish_ambiguous_remote` checks EN/JA with
  `company` and `personal` but no `origin`: the card names the proposed
  destination and both typed candidates, takes no lease, and Escape preserves
  both remote refs, checkout and the absent Push receipt. Pair it with
  `publish_remote_ambiguity_requires_choice_for_both_plan_producers` in
  `kagi-git`'s `push_test`: current/sidebar plans require a choice only for
  ambiguous publication; `origin` or a sole remote remains quiet.
  Also run `cargo test -p kagi-git --test push_identity_test`: execute the
  **original approved plan**, without replanning, after adding `origin`,
  changing the selected URL/pushurl, or renaming/replacing its remote. Both
  current and non-current sidebar publish must produce typed identity refusal,
  one durable Refused receipt, no upstream and unchanged refs/index/worktree
  on every remote. An unchanged approved ambiguity plan publishes once only
  to its named destination. Retain execution-time destination identity checks.
  Typed risk/blocker coverage retains explicit choices; warning-generation
  cleanup is independent of this admission policy. Tier B compares released
  versus fixed sidebar publish and non-current clean Pull in EN/JA, using
  disposable local remotes, identical viewport/theme/zoom, isolated profiles
  and only self-launched PID/window captures.
- Pull freshness (`tests/recovery/pull_freshness.rs`):
  `KAGI_GUI_E2E_ONLY=pull_freshness_,pull_current_` exercises a real held fetch
  before execution or a necessary choice, an advanced upstream unknown to the
  consumer, true synchronization, fetch failure, and same-owner joining.
  Native toolbar activation follows
  `keyboard_nav::keys` (keydown **and keyup**); toolbar ids are not bounds-registry
  controls.
  `pull_freshness_clean_updates` runs both that toolbar gesture and the actual
  current-branch context menu: right-click the measured `sidebar-local-main`
  row, then click its Pull item. Start each with cached zero and a separately
  advanced real origin; neither gesture may claim latest or change checkout
  before its held fetch succeeds. Safe plans then execute without a second
  confirmation. Local row measurements
  cover HEAD and non-HEAD rows only under `gui-e2e`, with no normal-build probe
  identifier allocation. Cached zero/unknown never disables healthy upstream
  Pull; busy/detached/remote/no-upstream guardrails remain intact.
  Compare staged content with `git ls-files --stage -z`, not raw index bytes:
  opt-in status refresh may repair stat metadata without staging a change
  (ADR-0193). Tier B uses two isolated clones and normal pushes to a dedicated
  remote: advance the producer without fetching in the consumer, click Pull in
  the owned app window, photograph stale `↓0` → fetched update → new HEAD without
  a safe-case popup. Also photograph a genuine no-op only after fetch success and
  an unreachable-remote fetch error; restore the fixture URL afterward. While
  fetch is held and on fetch failure, HEAD / staged content / working files must
  be unchanged. A failed fetch must never produce an up-to-date toast.
  The external HEAD / working-tree and opened-then-closed modal scenarios exercise
  confirmation invalidation and displacement. `pull_freshness_captured_reload_drift`
  drains the existing GPUI test dispatcher in background-only mode (`tick(true)`)
  to capture a real snapshot before mutating the checkout, then accepts that old
  read on the foreground dispatcher. Both HEAD-only and working-tree-only drift
  must invalidate the original retained risk choice; no new production seam is needed.
- reload keeping the open views (`KAGI_GUI_E2E_ONLY=survives_reload`): a commit's diff
  re-anchored to its renumbered row, the Compare pane and its file diff re-read, a
  Commit Panel file diff re-read (closed once nothing is left to show), and the
  Commit Panel kept while the working tree still has something to list.
- owner-stamped callbacks (`tests/recovery/remote_refresh_owner.rs`,
  `tests/recovery/fetch_owner.rs`, `tests/recovery/file_menu_owner.rs`):
  `remote_refresh_departed_owner`, `remote_refresh_newest_request`,
  `fetch_same_owner_piggybacks`, `fetch_different_owner_does_not_piggyback`,
  `fetch_owner_display_isolated`, `fetch_detach_retains_flight_and_isolates_reopen`,
  `file_menu_freezes_path`, `file_menu_rejects_stale_owner`, and
  `file_menu_focus_after_open_repository`.
  Use `KAGI_GUI_E2E_ONLY=remote_refresh_,fetch_same_owner,fetch_different_owner,fetch_owner_display,fetch_detach,file_menu_`.
  Remote tests queue a yielding transport task through `ui::e2e`, then exercise
  the real refresh launch and completion; no SSH process blocks the dispatcher.
  Fetch tests launch a real local fetch and switch/close before draining.
  File-menu tests defer the real panel callback, renumber rows, click the measured
  Discard control, deliver a retained action after changing owners, and assert
  that an invisible A menu cannot consume B's keyboard after `open_repository`.
  The focus scenario checks raw Enter opens B's Graph checkout plan and Escape
  closes a visible menu without clearing Graph selection.
  The existing `unmerged_branch_delete_armed` scenario also rejects delayed
  plans after departure and revisit while releasing the planning latch.
- guard writer abandonment (`tests/recovery/fetch_owner.rs`,
  `tests/recovery/guard_writer_panic.rs`):
  `KAGI_GUI_E2E_ONLY=fetch_panicked_worker_reconciles,fetch_previous_visit_is_not_presented,fetch_old_visit_drops_pull_waiter,fetch_new_visit_waiter_sees_old_flight_failure,remote_branch_fetch_panic,remote_branch_fetch_failed_after_departure,remote_branch_fetch_success_after_departure,pr_ref_fetch_restarts_after_revisit,pr_ref_fetch_panic,editor_save_panic,pull_confirm_departure_discards_old_visit`
  exercises the admitted background work's panic, Unknown receipt and reconcile
  notice, acknowledgement and fresh admission. `editor_save_panic_after_close`
  closes the pane before the panicked worker settles and requires one durable
  receipt, one live Operation Log row and exactly one toast; the open-pane
  `editor_save_panic` also requires one row and one toast. Departing and returning drops
  the old Pull waiter; a new visit's Pull joined to the old fetch gets its
  failure footer and toast, with one receipt and no old-visit confirmation.
  Failed fetches (including an old-visit fetch and a departed remote branch
  fetch) keep their frozen repository receipt visible in the shared Operation
  Log panel without changing the current tab's footer/toast or opening the
  panel. The remote branch success and failure legs inspect `klog::tail()` for
  the unchanged terminal lines after departure. PR activation retains its
  owned panes and intent while revoking obsolete visit publication rights;
  a current-visit ref fetch must reject the old answer without clearing its
  replacement's loading latch or replacing its head/files.
  `TestDispatcher`
  propagates an uncaught task panic before `Task::fallible()` can deliver `None`;
  the GUI-only seam catches the injected panic inside the worker and returns an
  absent result so the production abandonment branch can be inspected. The
  non-test path still uses `fallible()` for actual unwinds.
- #996 background writers (`tests/recovery/guard_writer_panic.rs`,
  `tests/recovery/app_conflict.rs`, `tests/recovery/app_stash.rs`):
  `KAGI_GUI_E2E_ONLY=snapshot_write_panic,snapshot_write_draws_while_busy,conflict_merge_continue_panic,conflict_confirm_continue_panic,conflict_skip_panic,stash_continue_panic,stash_continue_after_tab_switch,create_snapshot,stash_conflict_close_reopen,stash_conflict_followup`.
  Each panic scenario injects an absent worker completion through its own
  GUI-only seam, requires an Unknown/reconcile exit (and a durable conflict
  receipt), and distinguishes snapshot's GroupOnly scope from retained
  sequencer leases. The snapshot hold scenario draws a frame with its busy
  label while the write awaits release, then switches tabs and requires its
  unchanged `snapshot: created` contract line without a toast/footer on B.
  The stash tab-switch scenario requires the departed continuation's receipt
  without a toast or footer on B. Production mutations that fail each panic
  scenario: replace that writer's `abandonment.into_unknown()` conversion
  with an ordinary `GitError::Other` in its `fallible()` absent-completion path
  (snapshot, merge-stage, stash-continue, sequencer-confirm, and skip each
  fail individually). Removing `drain_unaccounted` alone is not a useful
  mutation: shared app-job polling also drains it. Dropping the snapshot hold
  before awaiting it fails the held-frame scenario; forcing departed
  completions through the presenting branch of `record_conflict_completion`
  fails the stash tab-switch scenario.
  PR #1008 follow-up Tier A:
  `KAGI_GUI_E2E_ONLY=conflict_confirm_dismissed_still_reloads,snapshot_failed_after_departure,conflict_merge_failure_contract`.
  The dismissed sequencer modal cannot suppress a successful current owner's
  reload and conflict re-detection; ordinary snapshot errors after leaving
  the tab reach a repository-named host notice without changing B's footer;
  current merge-stage errors show the normal footer/toast/bottom panel, and
  departed errors retain their oplog receipt, footer contract klog, notice and
  B's untouched footer/panel. Each holds or defers the real writer, not a mock.
  Three independent production mutations, each fails its own scenario:
  change the successful sequencer reload gate from `current` back to
  `modal_matches`; change the departed snapshot error queue gate from
  `!current` to `current`; remove the departed `record_conflict_completion`
  `klog!("footer: {}", footer_msg)` call.
- session-owned positioning and Smart Commit state
  (`tests/recovery/tab_ui_state.rs`, `tests/recovery/operations.rs`):
  `KAGI_GUI_E2E_ONLY=tab_ui_state_ownership,pr_open_enters_before_ref_fetch,smart_commit_generation_owner,smart_commit_modal_and_probe`.
  The position scenario preserves each tab's list/graph positions, paging limit,
  branch folds, and cleanup selection while proving a newly opened tab reads at
  its own default limit. The PR-open scenario proves an unfetched head enters
  its page before the background ref fetch settles. Smart Commit freezes the initiating session for spinner
  and completion state, drops detached-owner completions, uses the shared modal
  slot without Enter/Escape fallthrough, resets modal-list scroll on replacement,
  and probes repository-independent capabilities once per global revision.
- session-owned evidence (`tests/recovery/github_evidence_owner.rs`,
  `tests/recovery/cleanup_evidence_owner.rs`, `tests/recovery/conflict_evidence_owner.rs`,
  `tests/recovery/ecosystem_evidence_owner.rs`):
  `github_evidence_restores`, `github_evidence_background_owner`,
  `github_evidence_detached_owner`, `cleanup_evidence_background_owner`,
  `cleanup_evidence_superseded`, `conflict_detector_owner_guard`,
  `ecosystem_evidence_background_owner`, `ecosystem_evidence_superseded`, and
  `ecosystem_evidence_detached_samepath`.
  Filter with `KAGI_GUI_E2E_ONLY=github_evidence_,cleanup_evidence_,conflict_detector_owner_guard,ecosystem_evidence_`.
  `tests/recovery/evidence_support.rs` supplies yielding replies for queued
  transport tasks; production launch, owner capture, and settlement still run.
  PR restoration observes actual sidebar rows before the return-triggered fetch
  can finish; Analyze observes the existing `copy_diagnostic` clipboard output.
  Drain switch-triggered reads before setting an unrelated-tab sentinel, so a
  normal revalidation cannot masquerade as a foreign completion.
- scan read-revision safety (`tests/recovery/cleanup_evidence_owner.rs`,
  `tests/recovery/squash_evidence_owner.rs`):
  `KAGI_GUI_E2E_ONLY=cleanup_evidence_read_revision,squash_evidence_read_revision`.
  Both hold transport completion, create a real commit, and accept a new read
  while its owner is background. Cleanup checks rows, PR evidence, and the
  selection/delete-plan consumers on return. Squash compares commit/lane/edge
  signatures after returning to the owner, then proves a fresh scan still draws.
  Its test observer holds render-driven scan re-arming to model completion
  before the next scan launches; it never changes generation or read revision.
  Remove each `Reads::is_fresh` guard independently: only that scan's revision
  scenario must fail while the sibling and existing ownership scenarios pass.
- accepted-model generation safety (`tests/recovery/cleanup_publish_owner.rs`,
  `tests/recovery/squash_publish_owner.rs`):
  `KAGI_GUI_E2E_ONLY=cleanup_evidence_publish_generation,squash_evidence_publish_generation`.
  These start a scan after `Reads::begin`, then accept a changed model with the
  same `ReadKey`; request revision and scan generation deliberately stay equal.
  Cleanup observes fresh rows, PR evidence, and select/delete consumers. Squash
  compares commit/lane/edge signatures. The same scenarios cover rejected
  accepts, another-owner publication, `publish_tab_view`, and `amend_tab_view`.
  Removing only either publish-generation guard must fail only its matching
  scenario; the earlier read-revision scenarios must remain green.
- session-owned read caches and history (`tests/recovery/cache_history_owner.rs`):
  `KAGI_GUI_E2E_ONLY=read_cache_revalidates_on_activation,operation_history_session_and_stale_ref,background_operation_history_owner,welcome_drops_root_main_diff`.
  The cache scenario retains A's cache while B is active, changes A externally,
  then proves activation clears every stale payload before the owner full read
  publishes the new WIP/status snapshot. History scenarios prove A and B expose
  separate undo stacks, external ref movement is refused, background operation
  completion records only into its frozen attached owner, and detached completion
  falls through nowhere. The Welcome scenario proves the final session release
  removes its retained Main Diff before ownerless rendering.
- session-owned pane resources (`tests/recovery/pane_resources.rs`,
  `tests/recovery/app_conflict.rs`):
  `KAGI_GUI_E2E_ONLY=retained_pane_resources,conflict_background_owner,conflict_revalidates_after_external_abort,tab_ui_state_rejects_detached_writer`.
  Real File History, Analyze, Editor, Conflict, and Terminal entities freeze
  their owner. Background completions update only that owner's retained state
  and cannot touch another tab's pane, modal, or footer. Returning restores
  entity identity and edited buffers; returning after an external conflict
  abort clears the editor before the fresh read and rejects old-visit
  completions. Closing the owner makes every retained `WeakEntity` dead.
  Pair with `smart_commit_generation_owner`, `diff_survives_reload`,
  `compare_survives_reload`, and `fetch_detach_retains_flight` for Commit Panel,
  diff/compare refresh, and operation-lifecycle coverage.

For worktree-decorated branch checkout, scope
`KAGI_GUI_E2E_ONLY=graph_worktree_open`. The scenario double-clicks the actual
branch-name hitbox, then dispatches selected-commit checkout after reselecting
the row. On a clean fixture both plans must resolve the undecorated branch and
show the typed other-worktree occupancy blocker. Enter must not change either
worktree's HEAD/index/files; no untracked-file warning may mask auto-execution.
The existing tree-glyph navigation checks still run. Backend regressions in
`crates/kagi-git/tests/ops_test.rs` cover main/linked occupancy, occupancy arising after approval,
and successful checkout after the sibling detaches.

App keybindings, command-registry keybindings, and native menus share their
installation path with `run_app`; component initialization must precede that
installation so app bindings keep the same precedence. For menu/Enter routing,
PM can scope
`KAGI_GUI_E2E_ONLY=branch_menu_no_checkout_fallthrough,modal_no_fallthrough,remote_browse_modal_routing,window_modal_exclusivity,app_notice_modal_replacement,pull_failure_notice_waits_for_remote_browse,pull_failure_notice_waits_for_app_notice,pull_failure_notice_displacement_vs_dismissal,update_install_lifecycle`.
The menu and modal scenarios in `tests/recovery/operations.rs` select a non-HEAD
commit and prove Enter cannot fall through to checkout. Remote Browse also proves
that the shared workspace/Welcome modal-key wrapper routes Enter through both its
connection and Browse stages, routes Esc to the slot on both surfaces, and keeps
the workspace's vacant-slot Enter checkout fallback. The window-modal scenario
records the rendered Remote Browse, Update, and AppNotice overlays: an arriving
notice waits behind the occupied slot, Update consumes Enter without acting, and
Esc closes it. The AppNotice replacement scenario proves unrelated modal setters
preserve both actionable and plain unread notices. The production dirty-Pull
failure scenarios prove a recorded fetch failure never replaces a foreground
modal or extends an existing notice queue, stays durable in Operation Log, and
does not interfere with a delayed Acknowledge action.
The update-lifecycle scenario proves an installer remains window-owned while its
modal is closed, rejects a second start after reopening, renders a retained
completion, consumes Enter, and closes via Escape after the last repository tab
transitions the window to Welcome. Native foreground/input-focus behavior remains
a separate Tier B check.

The runner's screenshot capture is best-effort only; its assertions, clipboard
checks, refs, and persisted oplog records are the oracle. It cannot run from the
Codex sandbox because the required macOS `hiservices` XPC path is unavailable;
have the PM or a human run this lane.

The locked GPUI revision does not expose the complete, last-rendered hitbox
collection to `VisualTestAppContext`. Do not treat a scenario-specific recorded
bound (for example, a measured button or footer) as a generic hitbox dump. A
complete `id -> window-relative bounds` failure diagnostic needs a public GPUI
API. Propose missing APIs upstream and record the proposal in `docs/decisions.md`;
do not introduce a dependency fork for inspection (AGENTS.md / #1479).
Until an official API exists, use Tier A's state assertions and Tier B's
live-window inspection together.

### What reaches a real `InputState` and what goes around it (#516)

A scenario that passes says nothing about text entry unless the text went
through a real gpui-component `InputState`. Read the scenario's row here before
citing it as evidence for typing, pasting or a field's value. None of these
cover IME composition or AppKit's input method: Tier A delivers keystrokes and
paste actions, not marked text (use Tier B for IME).

| How text reaches the field | Scenarios (`KAGI_GUI_E2E_ONLY` names) | What it proves |
|---|---|---|
| Typed keystrokes into the focused real `InputState` (`simulate_keystrokes` with characters) | `conflict_save_boundary`, `editor_save_admission`, `editor_save_buffer_identity`, `editor_external_change_banner`, `editor_banner_rename_and_save`, `remote_connect_keeps_dirty_editor`, `cross_worktree_merge` (Editor buffer); `create_branch_presents_backend_receipt`, `create_branch_replan_error` (branch name); `issues_pagination` (list filter); `palette_push_modal_keys` (command palette); `workspace_mode_toolbar` (Issue title) | The key path: focus, the input's key handling, its change event and the product's sync from it. |
| Paste into the focused real `InputState` (`write_to_clipboard`, then `cmd-v` or `input::Paste`) | `remote_browse_escape_focus` (host), `worktree_lock_reason` (lock reason, after `cmd-a backspace`), `pr_fields_escape_focus` (picker filter), `workspace_mode_toolbar` (Issue title / body) | The paste path into the field the product focused, and the sync from it. |
| `InputState::set_value` on the real input (no key or paste event) | `conflict_continue_cache` (Result pane), `home_github`, `home_search_incomplete_repos`, `home_search_incomplete_refreshing`, `home_search_incomplete_work` (Home search), `theme_custom` (palette query), `pr_pagination` (sidebar composing text) | The input's change event and the product's handling of the value; not focus or key handling. |
| `InputState::replace` / `replace_all` on the real input (no key or paste event) | `issue_create_fields` (Issue body, via `insert_issue_body_for_e2e` / `replace_issue_body_for_e2e`), `home_work` (Reply body, via `insert_issue_reply_body_for_e2e`), `pr_same_number`, `pr_pagination_closed_tab_retention`, `pr_pagination_all_tab_retention`, `workspace_mode_toolbar` (PR composer; Issue body via `insert_issue_body_for_e2e` / `replace_issue_body_for_e2e`; Reply body via `insert_issue_reply_body_for_e2e`) | The input's change event and the product's handling of the replaced text (the Composer / Reply draft subscription included); not focus, key or paste handling. |
| `e2e::set_remote_browse_host_input`: `set_value` on the host input **and** a direct write of `host_input` | `merge_plan_latch`, `delete_branch_plan_latch`, `remote_browse_modal_routing`, `push_failure_keeps_modal` | Remote Browse holding its slot and input; not the form's own sync from the field (that is `remote_browse_escape_focus`). |
| No `InputState` at all: `e2e::open_local_panel_no_inputs` / `open_worktree_panel_no_inputs`, message from the `commit_msg` fallback (`e2e::set_commit_message`, as headless `KAGI_COMMIT_MSG`) | `wip_diff_survives_reload`, `commit_panel_survives_reload`, `worktree_wip_inline`, `worktree_panel_commit`, `worktree_panel_amend_discard`, `worktree_panel_discard_recording_failure`, `diff_highlight_once`, `diff_highlight_stale`, `file_menu_freezes_path`, `file_menu_rejects_stale_owner`, `file_tree_roles`, `hunk_staging`, `modal_compact`, `smart_commit_generation_owner`, `smart_commit_modal_and_probe`, `stage_failure_notice`, `dialog_a11y_roles`, `commit_stage_deferred_owner`, `commit_panel_revalidates_on_activation`, `smart_generation_close_drops_panel`, `commit_panel_refuses_during_activation`, `commit_close_drops_panel`, `manual_reload_releases_revalidation`, `commit_row_layout_wip` | The commit panel's ownership, staging and write paths. Nothing about the message or description inputs: these were built without them because each `InputState` registers an App-level observer that keeps it alive past its window (see `open_worktree_panel_no_inputs`). |

Not input, but also a stand-in: the `e2e::queue_*` seams (`queue_remote_refresh`,
`queue_remote_open`, `queue_smart_generation`, `queue_github_pr_fetch`,
`queue_github_pr_conversation`, `queue_cleanup_scan`, `queue_squash_scan`,
`queue_ecosystem_mine`) hand the next background read a task the scenario
controls instead of running `git`, `gh` or the LLM. They prove what the app does
with a result that lands late, for another owner or out of order — not that the
real read produces it. Used by `remote_refresh_departed_owner`,
`remote_refresh_newest_request`, `remote_browse_modal_routing`,
`smart_commit_generation_owner`, `smart_generation_close_drops_panel`,
`pr_list_roles`, `pr_suggestion_apply`, `pr_threads`, `pr_viewed`,
`pr_fields_escape_focus`, `field_picker_owner`, `ghe_viewer_login`,
`ghe_viewer_login_closed_only`, `github_evidence_restores`,
`github_evidence_background_owner`, `github_evidence_detached_owner`,
`pr_pagination`, `pr_pagination_races`, `pr_pagination_closed_tab_retention`,
`pr_pagination_all_tab_retention`, `pr_pagination_pending_closed_tab_return`,
`pr_pagination_pending_all_tab_return`,
`cleanup_evidence_background_owner`, `cleanup_evidence_superseded`,
`cleanup_evidence_read_revision`, `cleanup_evidence_publish_generation`,
`squash_evidence_read_revision`, `squash_evidence_publish_generation`,
`ecosystem_evidence_background_owner`, `ecosystem_evidence_superseded`,
`ecosystem_evidence_detached_samepath` and `workspace_mode_toolbar`.

`clone_picker_owner` (#1402, Clone side of #1420) uses
`e2e::queue_folder_answer` with `evidence_support::deferred` to substitute a
native folder dialog's `Result<Option<PathBuf>, String>`: late answers cannot
replan a replacement/reopened card; cancellation is silent; failures preserve
the card, report a localized error and record a failed receipt only when a
repository owns the screen. It does not validate an actual OS dialog.
`clone_completion_navigation` (#1405) uses `e2e::queue_clone_run` only to hold
dispatch: release runs the real local clone, verification and successful oplog
receipt. It proves background completion preserves the active session,
selection, focus and tab count until the drawn Open Repository action is
clicked (EN) or reached by Tab and activated with Enter (JA). Current-card
completion opens normally. Its matrix covers 940×660/1392×883, 100%/167% and
Apple Light/Dark. Use `KAGI_GUI_E2E_ONLY='clone_,home_,toast_'` for neighbors.
Actual folder sheets, occlusion and native focus remain Tier B.

The same kind of stand-in, outside `e2e::queue_*`:

- `KagiApp::queue_issue_list_fetch_for_e2e` (the next Issues list read, in
  place of `gh issue list`): `issues_pagination`, `home_work`.
- `remote_browse::e2e_transport::queue_remote_connect` (the next Remote Browse connection,
  in place of `ssh`): `remote_browse_escape_focus`.
- `e2e::worktree_inspection::queue` (the next worktree inspection, in place of
  the size and removal-condition read): `worktree_inspection`.

gpui's end-of-run leak detector stays on. `gui-e2e` enables `gpui/test-support`,
which enables gpui's `leak-detection`. The detector runs when the runner's App
is dropped, after the last scenario has passed, and a leaked entity fails the
whole run (`Leaked handle for entity …`, exit non-zero). That is why the commit
panel rows above use the no-inputs seam instead of keeping an `InputState`
alive. Do not get a scenario through by switching the detector off, keeping the
App alive, or forgetting a handle (`mem::forget`, `ManuallyDrop`, `Box::leak`).
If a real input cannot be torn down, use a seam and add the scenario to the
table.

## Tier B — real GUI driver

### Reusable modal/popup inventory (#1047)

`tests/recovery/inventory/table.rs` keeps the 74 historical #1013 targets,
their opening paths and fixture states, including explicit `skip` reasons.
Use the full stable name, for example `inventory:01-RemoteBrowse`; `inventory:`
selects the inventory, not the whole GUI suite. Inventory selections use the
existing KEEP_GOING supervisor automatically: one target per child process,
one window at a time. Ordinary scenario selections keep their previous behavior.
Mixed inventory/ordinary selections are rejected before output or window setup;
run them as separate commands so ordinary windows remain hidden/offscreen.

For matching product revisions, run these three commands in the indicated
worktrees (both must contain the inventory tool):

```sh
# Before: main worktree
KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='inventory:01-RemoteBrowse' KAGI_INVENTORY_OUT=/tmp/kagi-inventory-before cargo test -p kagi --features gui-e2e --test gui_e2e_runner
# After: feature worktree, same target, language, theme and viewport
KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY='inventory:01-RemoteBrowse' KAGI_INVENTORY_OUT=/tmp/kagi-inventory-after cargo test -p kagi --features gui-e2e --test gui_e2e_runner
bash scripts/inventory-diff.sh /tmp/kagi-inventory-before /tmp/kagi-inventory-after
```

Choose fresh output directories; an existing inventory is not overwritten.
Without `KAGI_INVENTORY_OUT`, output goes to worktree-local
`target/inventory/<timestamp>/`. `KAGI_INVENTORY_LANG=en,ja` and
`KAGI_INVENTORY_THEME=dark,light` request the four-image matrix; defaults are
`en` and `dark`. PNGs are `<name>-<lang>-<theme>.png`; `index.md` records each
captured target's opening path, image, or explicit skip/failure.
The parent appends a final failure row for unsuccessful children, including
timeout, abort/crash, startup and cleanup failures, even if the child could not
write its own result. A captured image is not proof of successful teardown.

For #1632, run `inventory:01-RemoteBrowse` alone with a fresh output directory,
then scope neighbors to
`inventory:01-RemoteBrowse,inventory:02-Clone,inventory:17-RenameBranch,inventory:48-EditorFsPrompt`
with `KAGI_INVENTORY_LANG=en,ja KAGI_INVENTORY_THEME=dark,light`. Require
the parent's successful exit and every child's PASS after PNG creation:
the end-of-context leak detector remains enabled. Record the locked GPUI and
component revisions alongside the images. Capture now calls the ordinary
shared `unmount` directly; do not replace the root with `Empty`, retain an
inputless `Root`, or add entry-specific success/leak exemptions. `shot` still
presents native frames before photography, and shared `unmount` drains the
native close callback; these serve screenshot presentation and NSWindow
closure, not the removed strong-InputState-capture workaround.

Inventory enables `KAGI_GUI_E2E_ONSCREEN` and `KAGI_GUI_E2E_VISIBLE` only for
its opt-in run, without focusing the window. Native `screencapture` needs Screen
Recording permission. These are screenshots of the real rendered root under
the Tier A dispatcher, suitable for visual before/after inspection; they do not
replace Tier B's live-input, focus, Accessibility or IME checks.

Composition is a separate command. It prefers ImageMagick `magick`, falling
back to macOS `sips` plus Python 3's standard library (no PIL). It matches PNG
names and writes `*-before-after.png` in the after directory; missing
counterparts fail before output. Missing tools report `合成なし` and exit nonzero,
without invalidating successful captures or altering their index.

Tool contracts are scoped with
`KAGI_GUI_E2E_ONLY='inventory_tool_selection,inventory_tool_matrix,inventory_tool_composition,inventory_tool_existing_runner'`.
They exercise target selection and mixed-selection rejection, parent-recorded
timeouts, actual language/theme capture, pixel-correct composition including its
fallback, and unchanged ordinary runner execution with hidden/offscreen windows.

### Live GUI interaction

Build `scripts/pidclick.swift` and, into the same directory, `scripts/pidcursor.swift`
(the agent cursor, below). Launch Kagi with a unique `USER` value, and retain
all three isolation flags. `USER` namespaces the per-user socket name, while
`KAGI_NO_RESTORE=1` and `KAGI_LOG_DIR` prevent fixture work from changing the
user's persisted session, settings, trust, or oplog. `KAGI_NO_ACTIVATE=1` avoids
stealing the foreground application.

```bash
swiftc scripts/pidclick.swift -o /tmp/pidclick
swiftc scripts/pidcursor.swift -o /tmp/pidcursor
VERIFY_USER="kagi-verify-$RANDOM"
VERIFY_LOG_DIR="$(mktemp -d)"
USER="$VERIFY_USER" KAGI_NO_ACTIVATE=1 KAGI_NO_RESTORE=1 \
  KAGI_LOG_DIR="$VERIFY_LOG_DIR" ./target/debug/kagi /tmp/kagi-vfx-a/repo \
  2>"$VERIFY_LOG_DIR/kagi.stderr" &
PID=$!
/tmp/pidclick windows --pid "$PID"
# Select Kagi's main window: the LARGEST layer-0 window of this PID, not the
# first one listed — Kagi also owns small layer-0 windows (title-bar strips,
# helpers), which pidclick refuses or the coordinates fall outside of. Set its ID:
WID=12345 # Replace with the selected window ID from the listing.
```

`pidclick drag <x1> <y1> <x2> <y2>` posts a left-button down, interpolated
held-button moves (at most 4 logical pixels apart), and mouse-up to that same
PID/window without moving the user's pointer. Use this for the conflict-line
checkbox paint gesture: start at the first checkbox centre and end at the third;
repeat from a checked checkbox to paint the same range clear.

For #1721, use independent EN/JA two-conflict repositories and a preserved
default-feature binary; record its source SHA, SHA-256 and actual `features=[]`
fingerprint separately from the GUI runner. Type the first Result and actually
confirm Continue. At the second conflict, verify physical stage 1/2/3 and fresh
markers, one unresolved file and unavailable Continue. Keep the bottom panel open,
exercise Terminal input and close/reopen it, and inspect Activity without overlap.
While the Result has focus and a selection, use real Cmd-R for repeated accepted
reads, then replace the selection without refocusing and check Undo/Redo. Clicking
the toolbar Refresh legitimately moves focus and is not a retained-input-focus
oracle. Type a distinct second Result, actually Save, verify its exact stage-0
blob, and Continue without restarting/reopening the Backend. Completion requires
the exact first replay as parent, the two intended commit bodies, clean index/WT,
the reattached branch and no active rebase directory; a leftover `REBASE_HEAD`
pseudo-ref alone is not the completion oracle.

Run the recording-failure case separately by holding only that private profile's
append lock during the first confirmed Continue. Capture the required stopped /
unrecorded acknowledgment, release the lock, dismiss it, quit and really restart.
Before restart a transient in-memory Partial row may still exist: do not call it
an empty history. After restart, show the genuine zero-row Operation Log and
inspect the preserved first body through its independent read-only action, without
applying it to the second conflict. Verify Git is unchanged until explicit new
input / Save / Continue, then finish the correct two commits. Also compare valid
identity-unknown legacy documents: inspect their preserved EN/JA bodies without
resolving the live conflict, then use Back to return to ordinary history.
Keep inspection open and use the real View→Language command for EN→JA→EN: header,
file/identity labels and readonly body must stay coherent without reopening the
repository or applying the preserved Result. Do not press either Copy on the host;
the matching dedicated/receipt payload checks run only on the private Tier A clipboard.
Dedicated/ordinary Copy consumer contracts belong to the existing Tier A private
`VisualTestPlatform` / `TestAppContext` clipboard coverage described above.
Never use the user's host clipboard for Copy/Paste, reads, snapshots or restoration,
even with an all-type snapshot or conditional restore. Actual OS clipboard
integration is permitted only in an isolated VM with no host clipboard bridge.
Capture only each launched PID's listed window with `screencapture -l<id>` and
close every owned application after the case.
For the operation-metadata boundary, retain a stopped sequencer's original
confirmation, change only its owned message or author-script in that private
fixture, and obtain a new explicit plan before accepting the changed input.
If the normal watcher retires the old modal before a click, record that limit;
do not claim ordinary stale execution from a dismissed confirmation. Held native
and public Backend consumers prove the prewrite refusal boundary. Verify the
actual final tree/message/authorship and retained Result after fresh approval,
not merely a successful button click or the absence of an error.



Three launch pitfalls, all seen in practice. Launch Kagi from an unsandboxed shell:
started from a sandboxed agent shell the process runs and logs normally, but
WindowServer never shows its window, so there is nothing to click. And with
`KAGI_NO_ACTIVATE=1` the window may not be on screen, so `windows --pid` (which
lists on-screen windows only) prints nothing even though the window exists;
clicks, keys and `screencapture -l` address the window by ID and still work, so
take the ID from `CGWindowListCopyWindowInfo([.optionAll], …)` for that PID.

**README images** (`docs/images/*.png`, #1049 / #1062) are 3104×2064: a
1440×920 pt window (`KAGI_WINDOW=1440x920`) plus the active-window shadow, on
the 2× built-in display. Three things change that size: a window opened on a
1× external display (1552×1032 — move it with System Events `set position`), a
window that is not frontmost (smaller shadow: 3016×1976 — activate it after
placing, then capture), and a locked screen, during which the window opens at
0.9× (`KAGI_WINDOW=1440x920` gave 1296×830 pt) and `screencapture -l` fails
with "could not create image from window". Check the size with `sips` after
every capture.

**Drop `KAGI_NO_ACTIVATE=1` when the scenario clicks the tab strip.** The tabs
live in the window's title bar, and macOS hands a title-bar click on a *non-key*
window to its own window-drag handling instead of the application. The click
never reaches the tab, the window moves under you, and the drag session it
starts swallows every later event — clicks stop working in the content area too,
and only relaunching recovers. Measured while verifying #643 Wave 4: two clicks
on the tab strip moved the window from `180,125` to `215,151` and killed input.
Without the flag, the same clicks switch tabs (`[kagi] tab-switch: <name>
cached=yes`) and the window stays put. Keep the flag for scenarios that stay in
the content area — it is what leaves the user's foreground app alone.

Dropping it has a real cost, so treat it as the documented exception to the
no-foreground rule at the top of this skill: `run_app` calls `cx.activate(true)`
right after `open_main_window` (`src/ui/mod.rs:3187`), guarded by exactly this
variable, so the launch pulls Kagi in front of whatever the user is doing.
`open_main_window` itself does not activate, and the Dock-reopen handler's own
`cx.activate(true)` is deliberately unguarded — the user asked for the window
there. Ask the user first, or run on a machine nobody is working on, and say in
the PR that the scenario needed the tab strip. Every other isolation flag
(`USER`, `KAGI_NO_RESTORE=1`, `KAGI_LOG_DIR`) still applies unchanged — they
protect the user's session, settings, trust and oplog, which foreground does not
touch.

`scripts/pidclick.swift` sends events with `CGEventPostToPid`; it does not move
the user's pointer or activate another app. Select a window with `windows --pid`,
then use its ID with `--window-id` and `move`, `click`, `rclick`, `key`, or `type`.
Coordinates are logical points relative to the window's upper-left corner. `click`
and `rclick` include a hover primer. `type` derives virtual key codes from the
active keyboard layout, so it is unsuitable for IME, dead keys, unsupported
characters, newline, or Tab input. Screenshots are physical pixels; do not mix
them with pidclick's logical coordinates.
Mouse events use explicit empty flags: stuck HID Control otherwise makes GPUI treat Left as Right.
Check suspected inheritance with `CGEventSource.flagsState(.hidSystemState)` (`maskControl` is 262144).

```bash
/tmp/pidclick --pid "$PID" --window-id "$WID" move 120 80
/tmp/pidclick --pid "$PID" --window-id "$WID" click 120 80
/tmp/pidclick --pid "$PID" --window-id "$WID" rclick 120 80
/tmp/pidclick --pid "$PID" --window-id "$WID" key 53
/tmp/pidclick --pid "$PID" --window-id "$WID" type 'filter text'
screencapture -x -o -l"$WID" "$VERIFY_LOG_DIR/window.png"
tail -f "$VERIFY_LOG_DIR/kagi.stderr" # [kagi] contract lines
```

**The agent cursor (#948).** `move`, `click`, `rclick` and `scroll` also show
where they act, so a person watching the run can follow it. pidclick sends one
datagram to `<per-user temp dir>/pidcursor/<PID>.sock` and, when nothing listens,
starts `pidcursor --pid <PID>` from its own directory (detached, its own
session), then waits about 0.3 s for the cursor to glide there before it posts
the event. When there is no cursor — `pidcursor` not built, or a daemon that
could not start (its stderr is kept as `<PID>.log` beside the socket, e.g. a
full disk) — pidclick prints one `pidclick: no cursor: …` line with the reason
and posts the event anyway. `key` and `type` do not move it.

- **It never takes the foreground.** The daemon is an accessory app that never
  activates, with one click-through, non-activating panel the size of the
  target window, ordered just above that window. It comes to the very front
  only when the target window already is the frontmost window, so it never
  covers another app. The panel joins every Space, so it follows the target
  window to whichever Space shows it; while the target window is off screen
  (another Space, minimized), the cursor is ordered out. It follows only
  windows of the PID it was started for.
- **It ends by itself:** when the target process exits (checked: killing the
  test Kagi ends it within 0.2 s and removes the socket), or after 10 minutes
  without a message (`PIDCURSOR_IDLE_EXIT=<seconds>` overrides, for testing).
  After 15 s without a message it fades out over 180 ms and comes back with
  the next one.
- **Screenshots do not show it.** `screencapture -l<WID>` captures only the
  target window, and the cursor is a separate window, so before / after images
  stay clean.
- **Turn it off** with `--no-cursor` or `PIDCLICK_CURSOR=0` (for timing-sensitive
  scenarios, where the 0.3 s wait matters).

Read `$VERIFY_LOG_DIR/operations.jsonl` as well as the visible footer and
`[kagi]` log. `KAGI_LOG_DIR` intentionally makes `src/main.rs::headless_mode()`
true, so this isolated launch does **not** create or forward through the
single-instance socket. The worktree command policy deliberately excludes
`KAGI_LOG_DIR` from its own headless-command check; the two policies differ.
Do not add `KAGI_NO_SINGLE_INSTANCE`: it was a headless-routing workaround before
#535 and is not the standard launch recipe. Do not remove `KAGI_LOG_DIR` just to
test socket forwarding, because that would risk the user's persisted state.

### Socket behavior, when it is the subject under test

In a normal GUI launch without a headless signal, the socket is named from
`USER`; a second `kagi <repo>` forwards an open-tab request and a bare `kagi`
forwards focus. Its receiver logs tab/open activity and raises the existing
window. This is separate from the isolated Tier B command above, whose log
directory prevents socket creation.

`cliclick` is historical and must not be used for new validation. It targets the
global pointer/frontmost application. `pidclick` needs Accessibility permission
for the responsible process; `screencapture` needs the applicable Screen Recording
permission. Use `screencapture -x -o -l<WID>` for a window-only image; the known
broken `-R` rectangle path is not a reliable substitute.

## Tier C — fixture states

Automated integration setup is owned by `tests/support` (#1704): `git_fixture.rs`
keeps hermetic commands, configurable remote seeds, explicit `clone_other` timing
and raw working-tree collection; `app_fixture.rs` keeps the serialized environment/
temporary-log/repository/request lifetime; `oplog.rs` filters records with caller-
specified tail limit and ordering. Backend suites use
`#[path = "../../../tests/support/…"]`. Keep snapshot types and receipt assertions
local: raw index bytes are not staged-content identity, and traversal options
retain each caller's nested `.git`/directory-symlink policy.

After changing these helpers, run the fixture/application targets with a fresh
`KAGI_LOG_DIR`:

```sh
KAGI_LOG_DIR="$(mktemp -d)" cargo test -p kagi --test git_fixture_isolation_test --test app_pull_test --test app_run_test
KAGI_LOG_DIR="$(mktemp -d)" cargo test -p kagi-git --test branch_sync_test --test force_lease_push_test --test fetch_remote_branch_test --test delete_remote_branch_test --test commit_index_identity_test --test pull_identity_test --test sync_to_remote_test --test pull_configured_upstream_test
```

Pair with Tier A `KAGI_GUI_E2E_ONLY='pull_,cherry_pick_presents_backend_receipt'`
and `KAGI_GUI_E2E_KEEP_GOING=1`, plus the workspace/invariant gates. Fixture-only
refactoring does not change real-window presentation and does not require Tier B.

Create the standard disposable repository only below `/tmp` (the script refuses
to overwrite an existing fixture):

```bash
bash scripts/make_fixture.sh /tmp/kagi-vfx-a
REPO=/tmp/kagi-vfx-a/repo
```

It supplies main/feature branches, a merge, a tag, origin, one stash, and a dirty
working tree — 12 commits, which is deliberately small.

A second argument deepens `main` with that many empty commits, inserted just
above the initial commit so the interesting shape stays at the top of the graph.
Anything that only happens on a long list needs it. Two different thresholds:
the commit list starts scrolling at a screenful (a few hundred is plenty), while
**paging** only begins past `DEFAULT_COMMIT_LIMIT` (10,000 — `COMMIT_PAGE_STEP`
is merely the 1,000 it grows by), so a paging check needs a five-figure fixture
(#737). Roughly 13ms per commit: 1,500 takes about 17 seconds, 10,500 about two
and a half minutes.

```bash
bash scripts/make_fixture.sh /tmp/kagi-vfx-deep 1500
```

For the three-stash menu and all four stash operations, consume the
initial dirty tree as stash two, then create stash three:

```bash
git -C "$REPO" stash push -u -m 'fixture stash two'
printf 'fixture stash three\n' >> "$REPO/a.txt"
git -C "$REPO" stash push -m 'fixture stash three'
git -C "$REPO" stash list
```

For the manual slow-remove check, create a linked `slowpre_remove` worktree and
commit this configuration there before opening the remove plan:

```bash
SLOW_WT=/tmp/kagi-vfx-a/slowpre_remove
git -C "$REPO" worktree add -b slowpre_remove "$SLOW_WT"
mkdir -p "$SLOW_WT/.kagi"
cat > "$SLOW_WT/.kagi/worktree.toml" <<'TOML'
[[pre_remove]]
type = "command"
run = "sleep 10"
TOML
git -C "$SLOW_WT" add .kagi/worktree.toml
git -C "$SLOW_WT" commit -m 'add slow pre-remove fixture'
```

The command step is a trust-gated precondition. Confirm its displayed plan, begin
remove, then attempt an editor save while it is running: the save must remain Busy
with its bytes and buffer preserved. A failed, untrusted, or headless-blocked
pre-remove command keeps the worktree; it never proceeds with removal.

| Family | Manual M check |
| --- | --- |
| Remove | Plan modal → execute → footer and oplog; while slow `pre_remove` runs, saving is Busy. |
| Stash | Exercise push, apply, pop, and drop against the three-entry fixture. |
| Stash conflict | Pop a conflicting entry → resolve → Continue → confirm the suggested drop path. |
| Conflict | Continue, then verify conflicts are re-detected immediately. |
| Backend policy | Toggle auto-snapshot OFF/ON; reset and amend must use the same policy from button and Enter. Guarded rebase keeps its existing no-auto-snapshot rule; explicit restore still creates its mandatory savepoint. CLI/MCP default to snapshots ON independently of GUI settings. Confirm create-branch with checkout ON/OFF: ON switches to the new branch with one backend operation; OFF keeps HEAD. Absorb with an index-write failure must show Partial and its original full OID, never a plain Failed after HEAD advanced. Worktree config added/removed/changed after the plan must refuse before target creation. |

### Parallel fixture isolation (#573)

Git fixtures use `tests/support/isolated.rs`: each parent test starts only its
own exact test in a child process with a fresh `KAGI_LOG_DIR`. The parent owns
the TempDir until the child finishes. This preserves normal parallelism without
changing the parent process's environment or sharing the user's oplog. Tests
that specifically exercise environment overrides may change them inside that
single-test child. Prefer explicit directory arguments for pure path tests.

The oplog refuses its default home path when runtime `CARGO_MANIFEST_DIR` is
present and `KAGI_LOG_DIR` is absent: `tests must set KAGI_LOG_DIR`. Do not remove
the Cargo marker or disable recording to make a fixture pass. Use the child
helper or pass an explicit storage directory through an existing API.

For an environment-race fix, run the following three times consecutively with
default test parallelism and record the results in the PR:

```bash
KAGI_LOG_DIR="$(mktemp -d)" CARGO_TARGET_DIR="$PWD/target" cargo test -j 8 --workspace
```

`-j 8` controls Cargo build jobs; it does not serialize Rust test threads. Do
not use `RUST_TEST_THREADS=1` to mask a race. GUI runner execution is a separate
lane and is not part of this fixture check.

### Process fixture ownership

Test cleanup must target only processes created by that fixture. Never use
name-based or command-line-pattern cleanup such as `pkill -f 'sleep …'`: a
matching process may belong to the user or another concurrent test. Keep real
deadline, group-stop and incomplete-I/O assertions; replacing the subprocess
with a mock or dropping those oracles does not prove the production boundary.

Own each fixture's privately published descendant PID and kernel start identity
for RAII cleanup. On macOS reuse `tests/support/proc_identity.rs`; Linux start
identity comes from `/proc/<pid>/stat`. The identity helper and helper-backed
cases retain their Unix guard and are explicitly restricted to macOS / Linux;
there is no always-failing other-Unix or PID-only cleanup fallback. Existing
broader Unix group-leadership, direct deadline and input / output tests remain.
The parent owns its root group only while its Child is unreaped; panic cleanup
can signal that group then, but after reap only a recorded descendant identity
may be signaled. Recheck kernel identity immediately before descendant cleanup.

Retain these real `proc::tests` oracles in `crates/kagi-git/src/proc.rs`:

- `wait_or_kill_stops_the_group_before_it_reaps_the_leader`;
- `run_child_deadline_is_not_an_exit_and_reaps_the_child`;
- `run_child_deadline_reaches_the_caller_despite_a_grandchild_on_the_pipes`;
- `run_child_clean_exit_with_unfinished_output_is_not_a_clean_capture`;
- `run_child_clean_exit_with_undelivered_input_is_not_a_clean_capture`.

A clean exit while a held-pipe descendant is alive must not claim group-stopped
proof. `proc::tests::fixture_cleanup_does_not_stop_another_same_marker_child`
uses two privately owned real `sleep 300` descendants: cleaning and reaping the
first leaves the second identity alive, then the second is cleaned and reaped.
The fixture publishes its PID + kernel start record atomically in a private
directory while the parent still owns its Child.
On Linux the regression first exits and reaps both direct fixture parents,
leaving two live descendants adopted by its private supervisor. Each cleanup
must reap only its own descendant, and the direct parents must remain available
to their owning Child handles. macOS keeps the original parent-owned child reap.

`proc::tests::owned_descendant_fixture_process` is an ignored, subprocess-only
helper requiring the private fixture environment; never select it manually or
run it with `--ignored`. Run the parent regression instead.
On Linux these four helper-backed cases re-execute individually in a dedicated
exact-test subprocess. Only that private subprocess enables
`PR_SET_CHILD_SUBREAPER`; the shared root harness is never made a subreaper.
Each fixture's reaper thread is outside the group being stopped and collects
only its privately published descendant PID with `waitpid(WNOHANG)`, after
checking the recorded start identity. It never uses `waitpid(-1)` or reaps the
direct Child owned by `run_child` / `wait_or_kill`. Linux identity probes retain
zombies until actual reap. Cleanup joins the reaper before deleting its private
directory. macOS keeps its existing inline fixture behavior.

The fixture's cleanup and reaper share one ownership mutex. Hold it across
kernel identity recheck + `SIGKILL`, and across kernel identity recheck +
exact-PID `waitpid`, so the reaper cannot release the old PID between a successful
identity check and the signal. Release it before `wait_until_gone`, poll sleeps,
finish or join. Panic/Drop cleanup must use the same guarded signal path.

Run `cargo test -p kagi-git --lib -- proc::tests::` for the scoped production
runner oracles and ownership regression. Linux runtime proof must additionally
run the same compiled test binary under a private Docker PID namespace with a
deliberately non-reaping PID 1: PID 1 waits only for its direct test-binary child,
not adopted descendants, and the container must not use `--init`. Compare the
original review head `8853fcff4e1b5e7e4653ce779a2f0f479c6d412a` with the fix,
recording test exit/assertions and exact adopted PID leftovers. The fixture
must reap its own descendants; do not skip Linux, accept a false failure, ignore
zombies in group proof, or weaken production `group_alive` / `group_stopped`.
Use private disposable containers, not a host service or global process match.

macOS evidence alone is not Linux runtime evidence. Record checks only after
execution; fixture isolation and clipboard deletion establish no measured speedup.

### Host clipboard safety

Never run the legacy `gpui-terminal` clipboard smoke tests on the host, including
from an older checkout to reproduce timings. Their creation / copy-paste / clear
tests connected to the shared OS clipboard, could overwrite or clear the user's
contents, and accepted failures or mismatches as success. All three tests and their
module are removed, not skipped or replaced with mocks; the production `Clipboard`
API and event proxy are unchanged. The two clipboard event-forwarding echo tests
are also removed: repeating an injected payload / event is not consumer selection
or Copy coverage. The default package lib suite is expected to contain 82 tests
instead of 87. It no longer attempts those host clipboard operations; this is not
a claim that every workspace test has been audited for host non-interference.

For app selection / Copy assertions, reuse GPUI's existing private
`TestAppContext` clipboard. The locked GPUI `VisualTestPlatform` used by Tier A
also stores clipboard items privately rather than forwarding them to the OS.
Existing consumer-visible coverage remains:

- `vendor/gpui-terminal/src/terminal.rs`:
  `test_bounds_to_string_extracts_selection` asserts selected grid text.
- `tests/gui_e2e_runner.rs`: `scenario_graph_copy` asserts the full commit SHA
  and local branch name through the app's Copy action.
- `tests/recovery/oplog_select.rs`: `scenario_oplog_detail_select_copy` drives
  a rendered text drag and ⌘C, then asserts before / after, the recovery command
  and recorded ref in the private clipboard; the expanded row must stay open.

The GPUI scenarios remain opt-in Tier A coverage, not default package tests.
None of these assertions prove arboard / OS clipboard integration. Exercise actual
system integration only in an isolated VM with no host clipboard bridge, using
assertions on the real outcome; do not introduce another fake clipboard or a new
GUI runner feature for this deletion.

## Other runtime seams

Use a real filesystem change to test the watcher and wait through its debounce:

```bash
printf 'x\n' >> "$REPO/a.txt"
git -C "$REPO" commit --allow-empty -m watcher-check
```

The launch-only hooks in `src/headless.rs` (`KAGI_SELECT_FIRST`, `KAGI_JUMP`,
`KAGI_CONTEXT_MENU`, compare, bottom-panel, terminal, menu-dump, and pull hooks)
are applied only at startup. They cannot drive an existing instance. There is no
headless hook for branch Solo; use Tier B's context-menu click.

The web/Playwright harness is a separate UI-story catalog and does not verify the
native app's backend flow:

```bash
bash scripts/build-web.sh
cd e2e && npm install && npx playwright install chromium && npx playwright test
```

Without the build the run stops at once, before any server starts:
`kagi-web harness not built: <files> missing in …/crates/kagi-web/dist`, with
`scripts/build-web.sh` to run (`e2e/harness.ts`, #516). It used to surface as
`Timed out waiting 60000ms from config.webServer`, which read like a runtime
hang. The check runs while `playwright.config.ts` loads: Playwright waits for
the `webServer` before it runs a `globalSetup`, so a check there would sit
behind the same timeout (measured: 61 s).

## Maintenance

When a PR changes a verification seam, environment flag, script, or runner feature,
update the matching part of this skill and state `skill 更新済み` in the PR body.
Adding a `tests/gui_e2e_runner.rs` scenario also requires updating Tier A's inventory.
Keep `.agents/skills/kagi-verify` as a symlink to the canonical source. Manually
run `uv run --project ci check-skill-refs` before merging. It verifies concrete
`scripts/*` and `tests/**/*.rs` references in inline code, fenced commands, and
Markdown links in this canonical skill (#544).

Finish by exiting the launched application so it cannot retain a socket or test
state. Fixtures and isolated log directories live below `/tmp`; remove them only
when their evidence is no longer needed.


### Backend executor visibility (#566)

`tests/support/backend_ops.rs` adapts legacy fixture signatures to Backend run or
a dedicated method, with the optional snapshot policy explicitly OFF. It must
never call raw executors or clear plan blockers. `tests/support/remove.rs` freezes
the opaque remove plan before any fixture drift. Compile-fail doctests guard old
root/ops/conflicts/staging/step-runner imports. D/F fixtures check owner trust and
frozen OID/child-list rejection through the dedicated Backend boundary.
`crates/kagi-git/tests/backend_fixture_storage_test.rs` drives the migrated branch adapter in
the shared isolated child, asserts a record in its log directory and preserves
a fake HOME oplog sentinel. Every new migrated fixture must use the same helper.

The low-level trust/headless acceptance cases are crate-internal tests in
`crates/kagi-git/src/ops/worktree_steps_acceptance_tests.rs`, with global env
changes isolated in child test processes. Run `cargo test --workspace`; GUI E2E
may be compiled with `--features gui-e2e --no-run`, but do not execute it for this
visibility refactor. Preserve the private safe-checkout and unapproved-config
oracles when updating fixture adapters.

### Ref-backed discard/remove recovery (#523)

Use `crates/kagi-git/tests/ref_backups_test.rs` for G: disable optional snapshots,
prune a disposable fixture with real Git GC, and recover the original bytes via
receipt `backup_refs`. A sibling naked blob must disappear to prove actual GC.
Check remove Success/Partial/Unknown and append failure. Explicit oplog retirement
must delete only its unshared roots; the final retained entry controls lifetime.
Default retention is indefinite, matching the oplog; snapshot pruning must not
expire backup refs. For M, inspect the receipt's ref and export its bytes before
choosing to retire that entry. Legacy naked-OID logs do not gain retroactive GC
protection. For Tier A, scope `KAGI_GUI_E2E_ONLY=worktree_panel,remove_public_boundary`:
the worktree discard scenario parses the blob from the existing `backup:` summary
and checks that the structured receipt ref resolves to that blob from both
worktrees. Ref names must not change existing executed lines or after/dirty text;
they have a separate `backup refs:` contract line. The same filter includes
`worktree_panel_discard_recording_failure`: hold the real oplog lock through
append timeout, check "changed but not recorded" and recover from the attempted
receipt, then repeat with a stale owner and require the owner-named notice.
G also covers a queued append after actual retirement, colon-before whitespace,
legacy id-less cleanup, and a pre_remove-created unreadable file: removal must
stop before deleting the worktree.


### Unmerged branch deletion (#584)

G in `tests/delete_branch_test.rs` covers the actual modal arm transition, merged
single confirmation, full-tip preflight refusal, unique reachability counts,
receipt-backed recovery after real GC, and commit-root retirement. It also covers
main/linked checked-out refusals (including clean worktrees and checkout after
planning), symbolic alias chains versus direct roots, and reflog removal before
branch name reuse. Tier A's
`unmerged_branch_delete_armed` scenario drives Enter and the measured button:
first confirmation keeps the branch/HEAD and records nothing; second deletes and
records its retained tip. Merged deletion stays one-stage. The same scenario
starts a plan on A, switches to B before completion, and asserts that B receives
no delete modal or mutation, releases the plan busy latch, and permits a fresh
plan on returning to A. It also holds the oplog sidecar lock to force an
append failure after deletion: expect a retained tip, owner notice and Partial
entry with its original `backup_refs` and owner metadata, no retry modal, and
the existing `async: delete-branch finished` log. G exercises this same display
conversion after a real sidecar-lock failure and retires a successfully recorded
branch receipt after passing it through the panel. PM runs only
`KAGI_GUI_E2E_ONLY=unmerged_branch_delete_armed`, with the usual isolated log directory.
When execution is prohibited, build it with `--features gui-e2e --no-run` only.
For M, compare EN/JA warning counts and armed labels, cancel/reopen to reset the
arm, then use the receipt's `git branch <name> <backup-ref>` recovery command.

### Release review: branch metadata and remove identity (#587)

G in `crates/kagi-git/src/ops/branch_delete_release_tests.rs` injects a test-only
transaction failure after reflog cleanup and checks the persisted Partial receipt,
retained branch tip and recovery ref. It also covers disabled reflogs, directly
written refs and non-NotFound cleanup errors. `backend/remove.rs` unit tests model
Unsupported creation time and verify all remaining fingerprint fields still
reject drift. These are fixture/unit checks; no GUI runner execution is needed.

### Release transport / Skip regressions

G: `transport_recording_test` validates the actual `mergedAt` query with fake gh;
`remote_stash_script_test::drop_forces_c_locale_inside_the_remote_shell` runs the
real script under a simulated translated Git; `conflicts_test::skip_advancing_to_the_next_conflict_is_not_a_failure`
checks that the next same-path conflict has no skipped draft. UI `transport_hold`
unit coverage verifies GitHub PR merge owner/operation isolation and Partial/Unknown admission;
SSH pull now uses the shared app lease, not this hold.
Skip の分類順 (#569 (1)) は `cargo test -p kagi --lib conflict_skip::tests`
で確認する。`tests/recovery/conflict_skip_g.rs` は GUI を起動せず、実 repository と
`Sessions` で Unclear の lease 保持・writer 拒否・reconcile 登録、typed
TerminationUnknown の Unknown 維持、既知結果の通常解放を検証する。
既存 `app_writer_admission_test` と `conflicts_test` の `skip_` も併用する。
For M, PR merge retains its transport hold after Partial; remote pull holds its
`WriteScope::Remote` lease during execution and retains it with a reconcile
requirement after Unknown/Partial/abandonment. Dismissing a notice or switching
tabs does not release either one. Remote pull's unobservable result requires
Inspect → arm → confirm and an audit row before the lease releases. Failed
releases normally. `KAGI_GUI_E2E_ONLY=remote_pull` (`tests/recovery/pull.rs`)
drives real fake-SSH `-G`, common-dir and agent-only planning, the blocked pull
and fetch, quit admission, Unknown notice and audited two-step release.
`remote_pull_preflight_refusal` changes the common dir after confirmation: no
`git pull` runs, Refused is recorded, and the lease releases.
`remote_pull_proxy_route_refusal` keeps the same hostname and common dir but
changes only `ssh -G` ProxyJump; the frozen route identity must refuse before
`git pull`, record Refused, and release the lease. Domain
`pull_route_changes_connection_identity_but_disabled_options_do_not` verifies
ProxyJump/ProxyCommand/ControlMaster/ControlPath and disabled normalization.
`remote_pull_toplevel_refusal` keeps the repository common dir but changes the
physical linked-worktree toplevel after confirmation; no pull runs, Refused is
recorded, and the lease releases. The real Git fixture binds the execution to
the frozen physical toplevel instead of the selected symlink, while the oplog
scope retains the selected root.
`remote_pull_branch_refusal`, `remote_pull_head_oid_refusal`, and
`remote_pull_upstream_refusal` change only the checked-out branch, HEAD OID,
or resolved upstream after confirmation. All must refuse without running
`git pull`, record Refused, and release the lease. The
`remote_pull_head_read_refusal` case also requires Refused when the preflight
HEAD read fails. `remote_pull_url_refusal` changes only the effective remote
URL after confirmation; the preflight must record Refused, release the lease,
and run no pull. Skipping the config comparison fails the zero-pull assertion.
Domain `pull_common_dir_requires_complete_physical_paths_head_and_git_config`
rejects missing/malformed Git configuration and preserves all fetch refspecs;
`remote_oplog_test::pull_probe_and_single_session_preflight_bind_the_approved_worktree`
exercises real Git against a throwaway repository: status, index, branch, URL,
fetch refspec, symlink-target and SSH-route drift each record one Refused
receipt without invoking pull.
`remote_pull_dirty_refusal` changes only the host's untracked worktree status
between the plan and preflight probe: it must record Refused, release the lease,
and run zero pulls; skipping the fingerprint comparison fails that assertion.
The same pure frame parser rejects missing staged/status delimiters while
allowing legitimate empty clean streams and preserving non-UTF-8 filenames.
The real-Git probe test changes unstaged content and then stages it, proving
that the two frozen digests change independently without HEAD/config drift.
`remote_oplog_test::remote_pull_merges_even_when_host_config_requests_rebase_and_ff_only`
creates divergent real-Git history with `pull.rebase=true`,
`branch.main.rebase=true`, `pull.ff=only`, and
`branch.main.mergeOptions=--squash`; the transport still creates a normal
two-parent merge commit and records Success. Removing the `-c
branch.main.mergeOptions=` override breaks that parent assertion.
`remote_pull_disables_host_autostash_instead_of_recording_conflicted_success`
overlaps dirty local content with an incoming tracked edit under
`merge.autoStash=true`: no implicit stash, no changed HEAD/worktree, non-Success
receipt; removing `--no-autostash` returns Ok despite a conflict.
`remote_pull_does_not_recurse_into_host_submodule_worktrees` proves that
`submodule.recurse=true` cannot move an initialized submodule's checkout;
removing `--no-recurse-submodules` moves it. The real-Git tests, not fake SSH
argv text, check the confirmed physical target and all pinned pull behavior.
The real-Git probe wraps `git status` and refuses unless
`GIT_OPTIONAL_LOCKS=0`; removing the export fails before confirmation.
The same fixture sets `TMPDIR` first to the approved worktree and then to
its Git common dir. Both must refuse before invoking `mktemp` or `git pull`;
the worktree must remain clean. Scratch outside the repository is removed
before the approved write begins.
`remote_pull_cached_preview_stale` changes branch before the live probe and
requires no modal/lease/pull plus localized refresh. G:
`cargo test -p kagi --test app_remote_pull_test` additionally checks cached
HEAD OID and dirty mismatch at plan time; Refused/Success/Failed release the
lease, while Unknown/Partial/abandonment retain reconcile. The prior injected
preflight test was removed: it could pass without exercising the remote script.
`remote_oplog_test::remote_pull_cannot_switch_origin_between_preflight_and_pull_connections`
uses two real bare Git remotes and redirects origin before a third remote
connection. Before #1014 the confirmed operation fetched and moved HEAD to
the unapproved remote; afterward planning opens one SSH and execution opens
one, so the redirect never occurs. `remote_oplog_test::a_non_zero_remote_pull_is_unknown_unless_the_refusal_is_recognized`
checks that an ssh-agent/publickey refusal records Failed with the reason,
while a disconnect or unrecognized response records Unknown. No managed
ControlMaster/socket or second-connection fallback is used, including on
Windows clients (system ssh to a remote POSIX shell); remote stash's
frozen-identity policy is unchanged.
For a real authenticated SSH fast-forward, the opt-in
`remote_ssh_live_test::live_remote_pull_fast_forwards` requires
`KAGI_REMOTE_TEST_HOST`, `KAGI_REMOTE_TEST_PORT`,
`KAGI_REMOTE_TEST_PULL_REPO` (a disposable checkout already behind its
upstream), and `KAGI_REMOTE_TEST_PULL_EXPECTED_HEAD` (the upstream's full
commit OID). Only set both pull-specific variables for a repository where
the test is allowed to write; the ordinary live read test stays read-only.

Continue の post-read (#569 (2)) は
`cargo test -p kagi --test app_writer_admission_test continue_` で確認する。
実 stash conflict の解決後、壊れた ref で summary を失敗させ、stage 済みであること、
Stopped Unknown・lease 保持・reconcile 登録を検証する。未解決 buffer の実行前拒否と
既知の `Staged` 成功は通常解放する。既存 `conflicts_test` / `stash_conflict_test` も併用する。
これは backend と app settlement の G 検証で、GUI の実行・表示確認ではない。

### PR merge local branch cleanup (#705)

G: `cargo test -p kagi --test transport_recording_test --test oplog_nonrun_ops_test`
exercises fake gh against real refs: retained deletion, plan-known checked-out /
PR-head mismatch keeps, late drift, a branch created after approved absence,
queued Success, and failed transport followed by a merged re-read without cleanup.
Fork Unknown reconciliation must acknowledge a merged PR with the local branch
still intact, then permit Kagi's ordinary guarded delete with a recovery ref.
No external force-delete is a prerequisite. One receipt owns both halves.

Tier A filter: `KAGI_GUI_E2E_ONLY=pr_merge_write_lease`. In addition to the existing
lease/hold cases, this drives fake-gh fork merges in EN/JA: Deleted, quiet Absent,
late drift Partial, planned Kept (checked out / not at PR head), and queued Kept.
Planning shows the existing loading state and no confirmable modal until settled.
Only Partial holds re-merge; queued and planned Kept release admission. For M, inspect the frozen local name/OID,
the fork-remote warning, and the localized deletion/refusal notice. Tier B remains
PM-owned when execution is reserved; compile the runner with `--no-run` in that case.
The same scenario attempts best-effort captures before confirmation and after
settlement, tagged `pr-merge-local-{En,Ja}-<case>-{plan,notice}`. Report
the explicit `skipped` diagnostic when this GPUI backend cannot render an image;
never present state assertions as screenshot evidence. A live GitHub PR may be
used to inspect the modal without confirming; do not consume a real PR to test
the post-merge notice.

### Remote source drag merge (#590)

Tier A filter: `KAGI_GUI_E2E_ONLY=remote_source_merge_into`. The scenario uses
`graph-remote-<remote/name>` and `sidebar-local-<branch>` control bounds to deliver
real drag events, confirms through Enter, and asserts one successful receipt with
HEAD/index/worktree and remote ref unchanged. Each scenario unmounts its window.
When PM owns execution, compile only with `--features gui-e2e --test gui_e2e_runner --no-run`.

G: `tests/drag_merge_test.rs` exercises the public Backend boundary with a remote
source and non-HEAD target, including another-worktree occupancy and changed tip.
M: drop a remote chip onto a non-HEAD local row; the plan must show its full ref,
OID and EN/JA last-fetch warning. Confirm that no local source branch is created
and no fetch occurs; fetch explicitly beforehand when current remote data is needed.

For a checked-out destination in another worktree, scope
`KAGI_GUI_E2E_ONLY=cross_worktree_merge`. It drags the fetched remote graph chip
onto the linked-worktree branch name, checks tab opening/reuse, cancellation,
late-plan suppression, dirty-editor discard/cancel and external destination
branch drift. Enter then performs a normal two-parent HEAD merge in the linked
worktree; its index/files and receipt must agree, while the dirty parent stays
unchanged. The scenario unmounts its window. Backend coverage in
`tests/drag_merge_test.rs` also rejects fresh and stale plans against a dirty
linked destination without touching either worktree.

### Staging failure delivery (#490)

G: `cargo test -p kagi --lib staging_failure` uses real index.lock and oplog
sidecar lock fixtures in isolated children; it compares single/batch failure
details in EN/JA, checks that trust refusal records without requesting a modal,
and that only a failed oplog append requires explicit acknowledgment.
Use a fresh KAGI_LOG_DIR as with all cargo tests.

Tier A filter: `KAGI_GUI_E2E_ONLY=stage_failure_notice`. The scenario covers editor
paths, panel file indices and batch buttons under index.lock, including a linked
worktree panel while the main tab remains active. Per attempt it asserts the
Failed footer with a short translated cause and Operation Log direction, **no**
toast / automatic panel opening / `AppNotice` for an attended attempt, one more
Failed oplog receipt for the owning repo with complete path/cause detail, and
byte-identical indexes. An admission denial records a Refused receipt without a
notice. The #1698 policy replaces the earlier duplicated footer/toast expectation.
M: hold index.lock, click Stage/Unstage from both surfaces, verify the actual
cause is visible and no success toast appears. If recording also fails, the
required acknowledgment is the sole primary, retaining the attempted failure and
append diagnostic without an extra ordinary result footer/toast. Failed/Unknown
must not become Success or be reclassified merely because append failed.

### One requested operation, one result (#1698)

Use `KAGI_GUI_E2E_ONLY='toast_stack_budget,stage_failure_notice,hunk_staging,pull_presents_backend_receipts,pull_failure_presents_only_the_decisive_receipt,pull_completion_drops,pull_freshness_,pull_current_,pull_unknown_,push_unknown_,pull_auto_stash_success,pull_auto_stash_failure_restores,pull_refuses_when_the_dirty_set_moved,host_dirty_guard_close_then_quit,clone_hidden_status,close_window_initial_home,editor_save_admission,stash_remote_drop,stash_public_boundary,slow_write_explained,fetch_,oplog_revert_eligibility,worktree_panel_discard_recording_failure,worktree_admin_result_owner,history_persists,conflict_continue_refusal_result_owner,conflict_continue_stopped_result_owner,conflict_continue_failure_result_owner,conflict_family_result_owner,conflict_merge_failure_contract,oplog_append_failure_conflict_stopped,conflict_confirm_continue_panic,conflict_confirm_dismissed_still_reloads,conflict_merge_continue_panic,conflict_skip_panic'`
with `KAGI_GUI_E2E_KEEP_GOING=1`. Real repeated no-op Push is footer-only when
attended; the existing `set_window_active_for_e2e` input selects inactive
presentation without activating an OS window, where the same result replaces one
keyed toast. Different result/repository keys retain separate cards. All Pull
child receipts keep their backend IDs, while fetch/stash steps produce no
independent completion and the workflow is named Pull. Failure previews have
localized causes and a log action without raw stderr/exit/absolute paths; complete
details remain durable. Hunk success has no toast. Quit/native-close repeats
coalesce in active/queued notices and disappear after real writer/clone settlement
without automatically quitting. Recording-failure acknowledge notices remain.

For ordinary no-op Push, use the existing enabled Repository Push command
(`Cmd-Shift-K` on macOS) and correlate each distinct input with its existing
`[kagi] plan: push blockers=... warnings=... preview_commits=...` line. Six
clicks on a disabled toolbar are not six executed requests. Confirm the typed
up-to-date result stays in one bounded current-context footer, with no modal,
toast stack or automatic log switch. Compare HEAD, all refs, index and tracked
file hashes before/after and verify no Push receipt was created. Keep this
ordinary producer witness separate from native inactive-window policy seams.

The toast budget scenario also starts real Fetch failures before entering Home or
another tab, then invokes the actual detail Button to reveal the same recorded ID.
Closed-owner and removed-worktree legs must paint the shared log, not only mutate
hidden panel state. The action is dismissed only after navigation succeeds.
The JA leg activates the native accessibility adapter before drawing and scopes
AppKit Press to the toast's named Status ancestor; the ordinary Operation Log tab
has the same caption and must not consume the detail-action regression.
`oplog_revert_eligibility` also clicks unavailable restore/revert actions in EN/JA
from Home, another repository, a remote view and after all owner tabs close:
the reason remains visible, no plan/write/receipt appears, and the same receipt
still copies its complete details. The ordinary matching-owner plan and existing
backend preflight remain the positive control.
Local owner fixtures use canonical paths as production startup does. Readonly
Copy must preserve complete ref names and old/new OIDs; do not require a repository
locator header absent from the existing clipboard format.
Long freshness-fetch no-op/write legs retain each request's own elapsed time,
including shared fetches; Unknown receipts override a Partial workflow preview.
The executed-Pull → no-op sequence checks actual replacement rather than a full
key-format string. Joined-flight legs first accept a rendered enabled branch-menu
intent, then deliver its production callback after the held Fetch acquires its
lease; a new toolbar click while that lease is held must remain unavailable.
Long branch results must retain the selected target in their final notification.
Run the EN/JA lock-classification boundary unit regression: an `index.lock` path
alone, a permission denial or a remote URL is not evidence of an existing lock.
Local Stash success coverage checks attended footer versus inactive/departed toast,
using session and visit identity with no stale Busy or unrelated-owner footer.
Explicit SSH Stash/Pull uses one bounded result toast independently of the normal
remote snapshot's Busy/read-only footer. Test same-visit, inactive, away and
same-session/new-visit delivery, one original backend row, canonical details and
recording-failure/Unknown priority. A skipped foreign refresh is intentional.
`clone_departed_result_owner` and `stash_revisit_result_owner` cover Clone's
actual result ID and local Stash visits; `remote_stash_revisit_result_owner` and
`remote_pull_current_result_owner,remote_pull_inactive_result_owner,remote_pull_away_result_owner,remote_pull_revisit_result_owner`
cover the corresponding remote boundary. Never infer an own-result action from
the latest unrelated toast or a positive-only receipt-ID assumption.
`remote_stash_result_survives_refresh,remote_pull_result_survives_refresh` executes
real disposable Git through the unchanged SSH protocol with owned identity and
known-host files, then completes the ordinary remote snapshot without intercepting
refresh. Check the visible result remains, the observed Git state reaches Graph,
and each actual request has only its original receipt. The Pull leg also executes
a second real no-change request through the production request handler and actual
confirmation; this proves that boundary, not disabled-toolbar/shortcut admission.
Known internal/CLI no-change summaries must agree in EN/JA and differ from unknown
successful stdout; retain the latter in the receipt, not the primary UI headline.
For ordinary remote smoke, the owned SSH adapter must start in its intended HOME,
not accidentally browse the invoking checkout. Wait for actual host-field focus,
inspect the exact owned host/root before Open, and complete normal refresh.
A genuinely enabled cached-behind Pull can encounter an already-current origin;
prepare any stale tracking state before the new plan, preserve all frozen
preflight guards, and observe actual canonical no-change output plus unchanged
HEAD/index/tracked files. A real freshness fetch may update remote-tracking refs.
Do not force a disabled control, alter state after approval, or equate a shortcut
attempt with backend execution. Local preflight no-ops create no write receipt;
an executed SSH no-change keeps its one existing backend receipt.
The worktree-admin matrix executes real Lock/Unlock/Prune/Repair with healthy and
failed audit append; history uses the actual reflog/quiet-reload producer, not an
assigned cursor. Conflict-family consumers cover ordinary known refusal as well
as success and failed recording for Save, directory/file resolution and Abort in
EN/JA, attended and inactive presentation. Do not preserve the old triple
AppNotice/toast/footer expectation.
Continue uses real multi-step rebase conflicts: typed Staged is stopped, not
whole-operation completion. Background delivery preserves the foreign repo's
footer/panel until an actual Details click; that explicit action intentionally
follows the canonical receipt owner. Scope window-global log assertions by the
actual recorded repository/request, not operation name alone.
`hold_next_continue_confirm_for_e2e` is a `gui-e2e`-only deferred slot after real
lease admission and before `execute_conflict_continue`. The failure consumer moves
only its owned Git directory to exercise a real backend-open KnownFailed outcome;
it checks healthy/failed append, current/departed ownership, actual Details/ack,
unchanged physical state and no duplicate receipt. Existing panic consumers
exercise injected Unknown delivery, not a real external-process Unknown. A matching
consumed frozen confirm must retire naturally on terminal completion; never force
the notice queue to drain only in a test or clear another/newer modal.
An unrecorded unchanged Failed row retains Failed and its original backend
diagnostic; only the existing successful-but-unrecorded display mapping uses
Partial. Verify absent trusted restore metadata and unchanged durable journal,
not an invented positive receipt ID or a copied translated message.
Tier B uses the same isolated EN/JA fixture and owned PID/window for before/after
no-op repetition, failure details, Pull and Quit. Launch with isolated USER/log,
no-restore/no-activate; foreground use requires permission and only the owned
PID/window. Report occluded byte-identical frames as stale, not new-screen evidence.
For recording priority, hold only the isolated audit sidecar during an actual
successful or stopped write; inspect original physical outcome, journal identity
and the sole required acknowledgment, then release, actually acknowledge and
perform a later operation. Initial merge/rebase and continued rebase stops retain
their observed stopped context; do not infer completion from backend Ok. Preserve
separate draft-lifecycle findings rather than clicking a misleading later Continue.
For normal conflict Save/Abort, inspect actual staged/recovered state and canonical
receipt. A subsequent Refreshed footer is not evidence that a green success footer
remained visible. A normal result must not add an acknowledgment notice.


### Presentation-only Busy progress close (#1719)

Select `busy_read_dismiss,busy_pull_progress_dismiss,busy_pull_revisit_request,busy_pull_merge_progress_dismiss,busy_hidden_failure_reaches_details,slow_read_explained,clone_hidden_status,queue_merge_plan_after_departure`
with `KAGI_GUI_E2E_KEEP_GOING=1`. These consumers use actual repository reads,
Fetch admission, queued planning and clone dispatch held by existing deferred
backend seams. The readonly `slow_reads_inflight_for_e2e` observer reports real
unfinished handle lifetimes, including superseded reads; it never creates work.
`busy_dismiss_focus` observes the rendered control's handle; keyboard coverage
must reach it through real Tab navigation rather than assigning test focus.
Exercise Enter/Space and actual AX Press. Natural read completion while close is
focused must remove the control and return focus through the existing root event.

Cover all reads already in flight, a newly admitted read, another owner and tab
round trips, read-phase retention, read/write overlap, the freshness Fetch → Pull
execution lease handoff, same-live-waiter join, in-flight departure/revisit with a
new-visit waiter, and a same-clock next request. Hidden failure must still
reach its exact durable receipt through the actual details Button. Existing Skip,
writer/quit guards, clone completion and stale queued-plan disposal stay active.

Initial requests use the actual enabled toolbar/branch-menu controls. Coalesced
join legs use the existing `handle_menu_command("repo.pull", ...)` boundary for
an already-enabled, delivered intent, while asserting the live toolbar remains
disabled. They do not demonstrate a fresh native click/shortcut during Busy or
a native input race; do not weaken admission to make these fixtures clickable.
`busy_failure.rs` covers EN/JA with the panel open and closed using real Cmd-J.
Record `(bottom_panel_open, bottom_tab)` before hiding, after hiding and after
failure delivery: only the actual Details click may switch to the exact receipt.
The historical `[kagi] bottom-panel: open (Failed auto-open)` line is preserved
for compatibility and is not evidence of a panel transition.

The revisit leg dismisses the new waiter again before departing; the flight must
not switch back to an older undismissed key. A new read on the other repository
must still appear. The conflict leg uses real competing commits, explicit Pull
Review and Merge approval, and verifies real unmerged index/Conflict Mode without
premature writes. `hold_next_merge_plan_for_e2e` reuses the former queued merge
hold slot for direct/queued planning; no production plan or request is fabricated.
It also cancels a separate normal Merge and checks a new normal request is visible.

Tier B uses an ordinary isolated binary, actual linked-worktree status work and
Terminal, EN/JA × Apple Light/Dark, desktop width ≥1000 plus the 940 compact leg.
After pointer/keyboard close, move focus to a different actual control (for
example click and type in Filter), then click the former card's Terminal-overlap
region and verify actual shell output in a file. Preexisting Terminal focus plus
typing alone cannot rule out an invisible hit target. Include the lower edge.
Record a still-live owned Git PID after close/input, or a held-read lifetime, to
distinguish dismissal from natural completion; a late screenshot alone cannot
establish that ordering. Then observe natural completion and the final result:
hidden failure and required audit acknowledgment must still arrive, and a new
same-name request must show progress. An intentional fixture transport delay is
not evidence for a real linked-worktree read's latency cause. Keep an independent
empty toast entity mounted so terminal results remain deliverable.

### Pull terminal settlement and strict-FF refusal (#1720)

Select `pull_ff_only_diverged_refusal,pull_ff_only_current_tree_consistency,pull_quiet_late_dirty_refuses,pull_presents_backend_receipts,pull_failure_presents_only_the_decisive_receipt`
with `KAGI_GUI_E2E_KEEP_GOING=1`. The diverged strict-FF scenario enters through
the actual branch-menu Advanced action. An unchanged watcher refresh must retain
the non-executable reason and disabled approval; Cancel must perform no write.
Dirty/HEAD/upstream/owner changes invalidate the explanation instead of making an
old blocked plan executable. Existing ready-plan preflight and late-dirty refusal
remain independent positive/safety controls.

Tier B uses disposable real repositories for FF, clean divergent Merge,
committed-history conflict and failed Fetch in EN/JA, desktop width ≥1000 and
the 940 compact leg. Record `pull.ff`, `pull.rebase`, per-branch settings,
configured upstream ref/URL and Kagi's actual chosen mode. Ordinary toolbar
Merge policy and explicit branch-menu ff-only are different inputs; do not
rewrite Git settings to make them agree or attribute all waiting to CLI policy.
Observe the real Fetch → plan/admission → result/refusal → lease/progress
retirement, including a subsequent actual Pull that is admitted normally.

Read actual Git HEAD, parents, index stages, file hashes and durable receipts
independently. A receipt's predicted `Success.after` is not a Git observation.
FF must reach the fetched tip cleanly; a clean Merge must have the correct two
parents. History Review is read-only until real Merge approval, after which
physical unmerged stages/paths and a stopped result must agree. A failed Fetch
must preserve checkout state and record its real Failed outcome.

For strict-FF, capture the initial reason, an actually observed unchanged
refresh and Cancel in each locale, with no write receipt. Separately induce a
real dirty or HEAD/upstream drift and verify expiry, preserved external state and
no automatic execution. Merely touching `FETCH_HEAD` is not proof that a watcher
read occurred. Registered `Cmd-R` also performs a quiet Fetch and can restore a
manually moved tracking ref; do not count that attempt as drift-expiry evidence.
Use an observed real watcher refresh with independently recorded tips instead.

For the terminal-read regression, compare the same kind of fixture with one
unrelated linked worktree holding 120,000 untracked files. Correlate existing
executed/verified log arrival and the real window, then verify actual Merge
parents, retired progress and next-request admission. Record the log reader's
resolution: same-interval arrival is not microsecond method timing or a portable
speedup ratio. This is not a reproduction of the user's 53-worktree repository
or a claim to have accelerated all linked-worktree reads. Do not extend product
timeouts, cancel tasks or scan the user's checkout to hide the distinction.

### Hunk staging (#842, Refs #357)

G: `cargo test -p kagi-git --test hunk_staging_test` (a 20-line file edited at
lines 2 and 18: one hunk staged / unstaged leaves exactly that edit in or out of
the index, the working tree untouched; a hunk drawn before the file moved and an
unstage of a range not in the staged diff are refused with `HunkChanged`; an
untracked file's single hunk is the file) and `cargo test -p kagi-domain
hunk_range` (header parse).

Tier A filter: `KAGI_GUI_E2E_ONLY=hunk_staging` (`tests/recovery/hunk_staging.rs`).
It clicks the measured `main-diff-hunk-stage-<row>` on the Commit Panel's
unstaged diff (index holds the first hunk only, panel lists the file on both
sides, the re-read diff shows one hunk), then on the staged diff in split view
(index back to HEAD, pane closed), then under a held index.lock (#490 footer,
index unchanged). M: open a two-hunk file from the Commit Panel, Stage hunk /
Unstage hunk in unified and split view (EN/JA labels), and check the panel and
diff update.

For #1233 / #1234 use
`KAGI_GUI_E2E_ONLY='diff_hunk_buttons,diff_keyboard_selection'` with
`KAGI_GUI_E2E_KEEP_GOING=1`. The first checks the pinned named ghost Button's
role/name inputs, Tab/F19 routing, Enter Stage / Space Unstage in EN/JA and
unchanged header bounds (original and new: 893×22.5 logical px at the fixture
viewport). The second enters the body by Tab before moving a caret, checks
header fill after final styling, and compares content-row selection fills after
Shift+Down ×3, retraction, collapsed caret, boundary extension, Cmd+A and
code-only Cmd+C. EN/JA unified/split use real backend rows; changing the row
count must preserve body focus without another Tab or click. Both ends of a
201-row diff remain inside the measured native viewport after keyboard
movement. Neither recorder is native VoiceOver evidence.
Tab shows the body ring, a measured header click keeps body focus without the
ring, and subsequent arrow input restores the keyboard-only ring.
Neighbor filter: `KAGI_GUI_E2E_ONLY='diff_,hunk_,peek_,copy_'`.
Tier B: Tab into the body and hunk actions; inspect focus, Enter/Space, tooltip
and live AX role/name, then Shift-arrow / Shift-Cmd-arrow / Cmd+A / Cmd+C /
Escape in unified and split. Compare identical EN/JA data, viewport and zoom;
selection Escape must leave the diff and copied notification visible.


### Nonblocking WIP opening (#1570)

Tier A: `KAGI_GUI_E2E_ONLY=compact_wip_` includes `compact_wip_first_frame`, holding the real opening
read before publication in compact and desktop windows. The placeholder and
compact composer must paint while held; releasing it publishes all 200 files.
A warm reopen retains the panel/title input and draft, and a newer status refresh
invalidates the held older result. A separately held Smart Commit capability
result exposes the localized detecting reason without falling back to rule-based
generation while an enabled provider is still unknown.
Global read invalidation without a following reload must automatically reread
and fill the pane. `compact_wip_probe_superseded` publishes synchronous status
before releasing the opening read and still requires provider discovery.
`compact_wip_amend_loading` checks compact EN / desktop JA unavailable help,
inert pointer and direct Amend dispatch while held, then an authoritative Both
plan after the staged payload arrives, with the repository unchanged.
Neighbors: `compact_select_changes,compact_layout_changes,compact_layout_state,compact_layout_lifecycle,smart_commit_modal_and_probe,smart_commit_footer_provider,worktree_panel`.
Use KEEP_GOING and the normal scoped native-run timeout.
Tier B click dispatch and sample evidence can identify synchronous work, but
an unchanged capture under shared-screen frame-delivery stalls is not paint
latency evidence. Use a native Tier A pane paint callback for first-frame timing;
`simulate_click` can drain later background work before returning, so timing its
whole return overstates the first frame. Temporary timing probes do not ship.

### Fresh compact Changes activation (#1662)

Tier A: `KAGI_GUI_E2E_ONLY=compact_changes_initial_file,compact_changes_deferred_file`.
The first scenario mounts fresh compact windows without visiting the WIP row:
both Staged and Unstaged must open their distinct index/worktree content through
pointer, Enter or Space across EN/JA, Apple Light/Dark and 100%/167% zoom.
The composer stays closed, and the same selected file and diff survive resize.
The second holds the real MainDiff read: a valid current result must land, while
another file, commit, Compare source, repository session or replacement worktree
panel retires the old result. Assert destination content, not just absence of a
panic. Normalize fixture repository paths when comparing macOS `/var` and
`/private/var` aliases; keep panel-entity and session identity checks.

Tier B: launch a fresh isolated ordinary app at 940×660, open Changes and click
a file label **before opening WIP**. Cover first Staged in EN/Apple Light/100%
and first Unstaged in JA/Apple Dark/167%, then switch to the other column.
Verify the named file and actual staged/worktree lines in MainDiff with no
composer. Resize the 100% case across the wide boundary and back; reopen Changes
and verify the retained selected file. Capture before/after windows. Do not click
Stage/Unstage/hunk actions merely to inspect a diff.

### WIP diffstat after staging (#996)

Tier A: `KAGI_GUI_E2E_ONLY=wip_diffstat_stage_order` in
`tests/recovery/hunk_staging.rs`. Hold the first scan after its background
diffs, stage again within the same cache epoch, and assert that the panel and
index already reflect staging while the first read is held. The second scan
publishes the final badge; releasing the first cannot roll it back. Mutation:
remove the `wip_diffstat_request` completion check in
`start_wip_diffstat_scan` and this scenario fails on the final badge.
Tier A `KAGI_GUI_E2E_ONLY=wip_diffstat_watcher_order` holds an older scan
after its backend read, edits the same already-modified file, and accepts
the watcher's newer diffstat without changing its cache epoch. Releasing
the older scan must not overwrite the badge. Every direct badge assignment
advances `wip_diffstat_request`, including reload, activation clear, tab
load, both watcher paths, and accepted scan publication. Production
mutation: remove the request increment from the status-changed watcher
branch; this scenario fails with the older badge.

### Busy snackbar labels (#607)

G covers EN/JA labels, unknown-tag fallback, and lease-mirror settlement without
clearing legacy plans. `uv run --project ci check-busy-labels` checks literal
busy tags and finite operation-name producers against the label table.
Tier A: `KAGI_GUI_E2E_ONLY=fetch_busy_label` in `tests/recovery/busy_label.rs`
starts a local fetch through `fetch_async`, checks the renderer's snackbar text
before completion in both languages, and checks release after completion.
Build only when PM owns execution. For M, verify Fetch, Commit, Stash, Discard,
branch deletion and editor Save show an operation label, never `app-writer`.

### Selection, hidden clone and scoped Home empty wording (#1451–#1453)

Tier A filter:
`KAGI_GUI_E2E_ONLY='pr_dashboard_hover_selected,clone_hidden_status,home_empty_section_wording,pr_dashboard_keyboard,clone_,home_'`
with KEEP_GOING. Dashboard coverage samples native selected-row fills before
and during hover in active/inactive windows, retains PR-number focus/AX identity
and checks one Enter/Space activation. Apple Dark's active and inactive tokens
can legitimately match; each state must still match its own resolved token.
The high-zoom setup sends a native wheel event inside the focused row to expose
the sampled neighbor without moving focus; distinct live bounds reject retired
virtualized-row probe coordinates.
The hidden-clone scenario holds production dispatch before a real fixture clone,
hides its card and opens repository B. The rendered Status names the frozen
source; Tab reaches the pinned Button's existing keyed focus owner and
Enter/Space restores the same request without another dispatch or lease.
Home pane/query changes cannot hide this entrance; only terminal-report delivery
removes it and releases the latch. Successful empty sections name owner/host in
EN/JA, including Enterprise and all-zero fixtures; failed owners retain failure
notes. Renderer AX/probe assertions do not claim native VoiceOver delivery.
Tier B remains PM-owned: compare selected-row hover and inactive fills, hidden
clone Show Progress on another repository and filtered Home, and mixed owner
empty/failure wording in real EN/JA windows. No percent or Cancel is introduced.

### Slow-read advice (#355, ADR-0206)

G: `cargo test -p kagi-ui-core -- slow_read busy` (the 2 s boundary and EN/JA
text), `cargo test -p kagi --lib -- slow_reads branch_menu` (per-phase timing,
Skip reaching the probe, unknown counts keeping Pull/Push enabled) and
`cargo test -p kagi-git --test snapshot_probe_test` (a skipped snapshot lists
the upstream with `counts: None`; the next one counts).
Tier A: `KAGI_GUI_E2E_ONLY=slow_read_explained` in `tests/recovery/slow_read.rs`
holds the real reload's snapshot in its ahead/behind phase
(`KagiApp::hold_next_snapshot_for_e2e`) and advances the dispatcher clock in the
tracker's 250 ms ticks: nothing at 1.75 s into the phase, the drawn
`busy-snackbar-advice` and `busy-snackbar-skip` after 2 s, a real click on Skip
hides them, the released read lands `counts: None` (status summary unknown, not
"no upstream"), and the next reload counts again — in EN and JA. Tier B:
`bash scripts/make_fixture.sh <dir> 10500`, add branches with upstreams, reload
and read the snackbar and `[kagi] busy: slow <op> after 2s` /
`[kagi] busy: skip <op>`; after Skip the sidebar shows `—`.
`KAGI_GUI_E2E_ONLY=worktree_size_not_explained` (#1012): the automatic worktree
size sweep, held on `e2e::worktree_inspection::queue`, stays "measuring" for
3 s with no `slow_read_shown` and no drawn snackbar advice / Skip; the released
measurement still lands on the row. Re-adding a slow read for the sweep fails
"a slow size measurement is not explained".

### Slow running writes (#355 stage 1, ADR-0206 amendment)

Tier A: `KAGI_GUI_E2E_ONLY=slow_write_explained,slow_read_explained,fetch_busy_label`
checks a real local fetch admitted on a lease, with backend dispatch held by
`KagiApp::hold_next_fetch_for_e2e` (`tests/recovery/slow_read.rs`). The dispatcher
clock advances in 250 ms ticks: no advice at 1.75 s, a drawn network reason and
`2 s` at the boundary, updated `4 s`, one
`[kagi] busy: slow write fetch after 2s`, no Skip or interruption warning, and
no advice after completion. While the write and snapshot read are both slow,
the write's explanation wins and the read's Skip remains available. The
neighbouring read scenario checks Skip alone. `cargo test -p kagi-ui-core
slow_write_advice` covers EN/JA reason classification and unknown fallback;
`cargo test -p kagi --test oplog_nonrun_ops_test a_panicked_run_job` and
`cargo test -p kagi --test app_writer_admission_test dropped_or_panicking_writer`
cover an abandoned run and a panicked guard losing their running explanation
without releasing an unconfirmed lease. Tier B: start a slow local fetch in an
isolated app, compare screenshots before/after 2 s and after completion, and
inspect klog. Remote SSH pull follows #989's lease migration; clone remains out
of scope.

### Operation queue (#355 stage 3a / 3b-1 / 3b-2, ADR-0204)

G: `cargo test -p kagi --lib app::queue` (the pure reducer: Q1–Q13 rows,
stage 3a's untracked-write, external plan, withdrawn-confirm and strip-removal
events, plus stage 3b-1's input-focus wait and blur release).
Tier A: `KAGI_GUI_E2E_ONLY=queue_` with `KAGI_GUI_E2E_KEEP_GOING=1`
(`tests/recovery/op_queue.rs`). `KagiApp::hold_next_run_for_e2e` (`gui-e2e`
only) holds the next admitted run-family write (checkout, commit, …) before
its backend work, with the lease held. Scenarios drive the product entry
(`dblclick_checkout_branch` → `start_checkout`) and read the strip through
`queue_strip_for_e2e(now)`:
`queue_runs_in_order` (strip row and `Queued: checkout b` toast at once, a real
click on `queue-strip-remove-<id>`, the queued checkout runs only after the held
one settles, with no modal on a clean replan, and its row reads `running · 2 s`),
`queue_trip_lists_cancelled` (the held checkout fails, both successors are
listed as `previous step failed` until `queue-strip-clear`),
`queue_confirms_a_warned_plan` (detached commit checkout retains its genuine
warning and queued modal; confirm runs one target, Cancel declines the next),
`queue_strip_owner_only` (tab B never draws tab A's strip; A's head waits for A
to return, and the queue hears of A's held activation read before A itself
through `sync_queue_for_e2e`), `queue_skips_auto_fetch` (a quiet fetch is
skipped while the tab has intents), `queue_rejects_during_untracked_write` (a
checkout behind the tab's own manual fetch keeps the old `OpInProgress`
refusal), `queue_resumes_after_reconcile` (`KagiApp::panic_next_run_for_e2e`
kills the held checkout: Unknown, its successor cancelled; once the reconcile
is acknowledged a later queued checkout runs), `queue_accepts_after_idle_fetch`
(a fetch that ended while the queue was idle does not refuse the next intent)
and `queue_refuses_a_blocked_checkout` (a dirty-tree blocker no longer prevents
busy enqueue; its fresh head plan refuses without writing), plus #1028's
`queue_checkout_transient_blocker_clears` (a staged overlap initially blocks
checkout, the held predecessor commits it, then the fresh clean checkout runs),
`queue_busy_checkout_preserves_local_changes` (idle checkout safely carries
unrelated staged and untracked work without a caution modal; busy shows the
strip at once and preserves those contents after replan. Untracked target-path
collisions instead produce a concrete-path blocker before idle admission) and
`queue_checkout_missing_ref_refuses` (a missing ref is statically rejected with
the exact bounded active-window footer or one inactive-window error notification,
never both, while the complete blocker remains in a durable refusal receipt), and
`queue_checkout_linked_worktree_refuses` (a branch checked out in another
worktree cannot enter the busy queue; its typed blocker reaches the oplog).

For #1777's menu/queue regressions, scope
`KAGI_GUI_E2E_ONLY='input_code_context_menus,input_names_inventory,file_menu_rejects_stale_owner,queue_merge_refusal_is_shown,queue_checkout_missing_ref_refuses,queue_checkout_linked_worktree_refuses,pr_peek_dirty_guard,pr_peek_read_failure_context,palette_push_modal_keys'`.
The Input menu, accessible-field inventory and PR Peek dirty-editor helper
explicitly select Edit for their Markdown fixture; Preview has no painted code
Input. PR Peek still dirties that real Input with a simulated keystroke and checks
`any_dirty()` before exercising the dirty guard and read-failure preservation.
The palette Push focus scenario exercises both window-activity states: raw Enter
must close the blocked plan, persist exactly its no-remote blocker and display
the exact bounded active footer or one inactive error notification, never both.
Raw Escape must still close the next plan without writing another receipt.
The menu's right-click hits before the selected glyph's
midpoint so nearest-caret rounding cannot collapse the exclusive-end selection.
The stale-owner menu scenario awaits the reopened
panel's asynchronous rows before freezing its menu, but still switches owners
before the deferred callback is delivered.

For #1696 review regressions, scope
`KAGI_GUI_E2E_ONLY='pull_ff_only_current_tree_consistency,queue_busy_checkout_preserves_local_changes'`.
The strict ff-only card shows its own EN/JA commit/stash-and-retry overlap note,
never buttons offered only by ordinary Pull. Cancel and execution refusal retain
the dirty files, index and refs. Checkout distinguishes unrelated untracked files
from target paths that safe checkout would overwrite; the latter refuse before
execution. Both idle double-click and queue use typed checkout admission: unrelated
dirty work stays quiet, while unknown cautions require confirmation and blockers
refuse. Backend checkout tests also cover file/directory collisions, an empty-tree
baseline for unborn HEAD, and paths unchanged between HEAD and the destination.
Also exercise staged child-vs-target-file and staged file-vs-target-directory
obstructions, plus a purely staged rename whose source is rewritten by the target.
Do not add an unstaged edit to that negative rename fixture: its status entry can
expose the source separately and mask missing `Renamed::from` handling.
Idle double-click and queued admission must show the concrete-path blocker before
starting a target writer; a disabled confirm must leave refs, index, bytes and
receipt count unchanged (only an existing queued predecessor may have run).
The same entrypoints must execute a nonoverlapping staged rename quietly and
preserve its index entry and split unstaged bytes. Backend `ops_test` counterparts
use the public `Backend::plan` for branch and detached checkout and assert read-only
planning, including component-prefix neighbours that must not be mistaken for
file/directory collisions.
After #1697 integration, also include `entry_consistency_` in the scoped run.
Current ff-only and non-current Pull must execute without a card for clean and
unrelated dirty fixtures, while unknown typed cautions retain their choice. In
Tier B, take fresh EN/JA captures from the integrated default-feature binary:
perform the disposable fixture's quiet Pull/checkout/publish actions, inspect
receipts and retained dirty bytes, then capture neutral tag/worktree previews and
zero-replay rebase without confirming them. Retain the released before captures.

#1032 adds `queue_commit_checkout_busy_replans` (direct busy
`open_checkout_commit_modal` freezes the target OID; the head plan reads the
predecessor's new HEAD; an external HEAD change after confirm makes preflight
fail with a durable `Preflight` receipt rather than executing a stale checkout)
and `queue_dirty_commit_enter_refuses` (real Enter on a dirty selected commit
while a write is held keeps the queue empty, starts no stash/checkout, and
shows a dedicated EN/JA two-write explanation in footer and error toast).

Stage 3b-1 adds:
`queue_commit_runs_after_checkout` (held checkout, staged change, queued commit
strip row and toast, then HEAD advances on the new branch with the frozen
message and no modal),
`queue_commit_confirms_changed_draft` (a changed draft opens `QueuedCommit`;
confirm uses the frozen message and preserves the newer draft),
`queue_commit_confirms_changed_staging` (a staged-set digest change opens
`QueuedCommit`),
`queue_waits_while_input_focused` (a drawn, focused Commit Input blocks the
head with `waiting: typing` until blur), and
`queue_dismissed_input_modal_releases_head` (a create-branch Input is closed
through real Escape without refocusing; the queued checkout proceeds after
the modal leaves the dispatch tree; dropping the focus-membership check
reproduces the stuck head). #1020 review additionally covers
`queue_commit_consumes_origin_draft` (checkout changes branch; the origin
draft is cleared and the destination draft survives),
`queue_commit_rechecks_staging_on_confirm` (a second external `git add` while
the modal is open cannot be committed by the old approval; reasons are drawn),
`queue_commit_refuses_late_merge` (external `git merge --no-commit` before
confirm keeps MERGE_HEAD and HEAD and cancels the queued plain commit), and
`queue_commit_detached_successor` (confirmed detached checkout then queued
commit, verified detached HEAD OID and next checkout's admitted successor).
The Commit Panel busy path releases title-input focus itself, so the clean
`queue_commit_runs_after_checkout` scenario never calls `focus_root`.
`cargo test -p kagi-git staged_set_digest` additionally exercises gitlink
mode and conflict index stage changes; `merge_in_progress_reads_external_state_after_conflicts_are_resolved`
probes the resolved merge guard.

For #1649 additionally run `KAGI_GUI_E2E_ONLY=compact_commit_preserves_new_draft`.
The compact native scenario holds a real commit, queues checkout, edits and focuses
the retained input, then releases the commit. Across EN/JA, Apple Light/Dark and
100%/167%, the newer draft stays drawn and focused, checkout waits for typing,
and dismissing the composer opens fresh confirmation. The matching consumed
draft instead clears and closes normally. Tier B repeats with a private bounded
pre-commit hook in an isolated repository: type again without refocusing after
success, remove an individual queued successor, dismiss the popup, and verify
fresh confirmation and retained unsubmitted work. Never hold a user's hook or
add a write-cancellation route.

For #1650 additionally run `KAGI_GUI_E2E_ONLY=queue_history_`.
Across EN/JA × Apple Light/Dark × 100%/167% at 940×660, two failed held
predecessors produce 32 distinctly named cancellations. The scenario wheels
through every actual row inside the viewport, keeps Graph and the status bar
usable, and reaches the last Remove among 16 pending rows beside that history.
Pinned Cancel all leaves the running lease held; Clear removes only history.
It also forces the native Always policy, wheels to a deep offset, drags the
actual thumb to reveal the newest row, then clicks the lower track to return
to the oldest. Wheel-only coverage misses a scrollbar attached to translated
content: keep the bar on the unscrolled `with_vertical_scrollbar` wrapper.
The production strip uses one shared scroll viewport, capped at the lesser
of scaled 240px and 20% of window height, with session-keyed scroll state.
`queue_history_drag_owner` gives two real repositories overflowing cancellation
lists, presses A's thumb, switches through production `switch_repo` while the
button stays held, then moves over B. B's newest row must remain at its original
position and both repositories retain HEAD/index. Scope the entire strip's
element subtree by session incarnation: separate handles alone do not isolate
the kit scrollbar's retained drag state.
Tier B repeats the eight conditions at 1000×700 using an ordinary binary and
private bounded failing pre-commit hooks, not the native hold seam. Capture
32 cancellations, the scrolled tail and Clear; compare HEAD, refs, staged
content, working files and failed receipts before/after. Also exercise
individual Remove and pinned Cancel all with 32 history entries plus 16
pending operations, observing that the admitted write continues until its
own hook is released. Duplicate frozen commit messages in this real fixture
are expected; the native case independently proves reachability of all 32 IDs.
Repeat thumb dragging and lower-track clicks in the ordinary app, using the
owned PID/window and current screenshot coordinates. Its system auto-hide
policy is separate from the native forced-Always assertion; process-local
`AppleShowScrollBars` launch arguments do not prove the kit adopted that policy.

Stage 3b-2 adds `queue_replans_merge_after_predecessor` (held commit changes
HEAD; live merge kind changes from fast-forward to merge commit),
`queue_confirm_departure_requeues` (A's queued merge modal withdraws on
departure while B's checkout proceeds, then A's intent reopens),
`queue_merge_conflict_trips_successors` (Conflict Mode leaves no new commit,
so the queued checkout is listed `previous step failed`),
`queue_merge_into_keeps_frozen_target` (checkout moves HEAD to `other`;
merge still updates the frozen `main` tip), `queue_drag_merge_while_busy`
(drag enqueue instead of busy refusal), `queue_menu_merge_while_busy`
(a measured click on the busy sidebar Merge action queues the frozen merge;
restoring the busy-disabled menu state makes this scenario fail),
`queue_confirm_blocked_merge_refuses`
(Enter on a blocked queued merge records a refusal and lists `plan failed`
instead of leaving the head `Admitting`), `queue_merge_plan_after_departure`
(a merge plan held until just after a tab switch the ticker has not yet seen
is discarded, logged as `queue: plan discarded`, neither opens nor reports on
tab B, and A asks again on return) and `queue_merge_refusal_is_shown`
(a genuinely warned detached checkout holds the queued confirmation with no write;
after an external detach, toast and footer show the HeadDetached merge note).
Unrelated carried edits intentionally do not create a checkout warning (#1696).
The merge modal is always
required, and merge planning owns the plan slot through its terminal callback.
Scope the full neighboring GUI coverage to
`KAGI_GUI_E2E_ONLY='queue_,merge,drag'`; do not launch it without a filter.
Tier B for stage 3b-2: queue checkout → commit → merge behind a slow write,
check each confirmation and `waiting: typing` on an input, the 32-entry
cancel list, and English/Japanese in an isolated app. The executable recipe
is prepared at `/tmp/kagi-944/tierb-355-s3b2.md`; its app run is separate.

Tier B: queue two checkouts behind a held or slow checkout in an isolated app
and capture the strip, cancel list, and running row's seconds in EN/JA; queue
a commit behind a held checkout and check its frozen message, confirmation and
input-focus wait in the real window.

### Background writer elapsed advice (#996 with #995)

Tier A: `KAGI_GUI_E2E_ONLY=slow_write_snapshot,slow_write_conflict_continue`
in `tests/recovery/slow_read.rs` holds the real snapshot and merge Continue
background workers before their Git writes. Each checks the admitted lease and
operation-specific snackbar label, a drawn reason plus `2 s` at the dispatcher
clock's 2-second boundary, `4 s` while held, and no advice after release.
Snapshot checks EN/JA and the ref only appears after release; merge Continue
checks conflict-resolution advice and the commit panel after staging. Their
`reserve_write` calls already register the kind and lease-start clock through
`mark_write_busy`, as #992's branch fetch does. Restore-snapshot is classified
`SlowWriteWorktree` rather than `SlowWriteStash`: restoring checks out the index
and worktree. Production mutation: subtract one second from the elapsed seconds
in `KagiApp::slow_write_shown`; `slow_write_snapshot` fails on the 2-second
drawn advice even though the lease and backend operation still run.
