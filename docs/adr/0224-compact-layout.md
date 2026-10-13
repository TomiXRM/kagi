# ADR-0224: A″ compact Graph layout (#1225)

- Status: Accepted for production after user approval of the prototype; verification pending
- Date: 2026-10-12
- Refs: [#1225](https://github.com/TomiXRM/kagi/issues/1225), [modern UI](../ui/modern-ui.md), [architecture](../rearch/architecture.md), [ADR-0183](0183-session-owned-read-model.md), [ADR-0093](0093-active-modal-enum.md), [ADR-0125 toolbar overflow amendment](0125-apple-hig-themes.md#amendment--2026-10-11-layout-owned-toolbar-overflow-1210)

The PR base is the long-lived `feature/compact-layout` branch, not `dev`.
Implementation is delivered as independently functional commits: breakpoint/
rail/Popover, Changes lists, pinned WIP composer, and shared-state resize proofs.
This ADR records the complete approved design; the first commit's changelog and
native gate report cover the navigator slice only.

## Context and roles

A narrow Graph workspace must reserve its width for history instead of squeezing
it between the full navigator and Commit Panel. The approved A″ prototype becomes
the production layout, not an alternative toolbar or a second commit workflow.
The navigator, graph and file lists retain **dense** roles; the composer retains
the existing **form** role. No global density or theme change is implied.

The references are macOS Mail's row-hover action reveal and Finder's quiet,
continuous sidebar hierarchy. Take the reveal affordance and navigation hierarchy,
not Mail's compose lifecycle, Finder's materials, or a claim of native-control
parity. Reuse existing Kagi sidebar section/filter rendering, keyboard/focus and
list helpers, bottom `TabList`, file controls, composer inputs/buttons, and the
pinned gpui-kit `Popover`; do not hand-build parallel controls or state owners.

## Decision

### One breakpoint, unchanged wide layout

Use one central compact predicate: **logical window width < `px(1000.)`**, shared
across all presets and independent of product zoom. At exactly 1000 logical px
and above, retain the existing wide layout with no visual change from this
feature. This supersedes the initial `theme::scaled_px(1000.)` proposal: Kagi's
100% base would make that threshold 900 logical px and exclude the accepted
940px compact viewport; 167% would incorrectly make the accepted 1392px desktop
compact. Breakpoint membership is a viewport decision, not a control-size token.
The rail remains `theme::scaled_px(44.)`; popup geometry and spacing continue to
scale through existing theme helpers.

Below the breakpoint, replace the inline navigator with that rail. Rail actions
open gpui-kit Popovers containing the **existing** sidebar sections and filters.
Navigation selection, filtering, empty/loading/error states and their operation
callbacks remain owned by the existing navigator/session models.

Keep `toolbar_display`, both toolbar presentations and the existing overflow
fitting/menu contract. Compact layout must not force Icon Only or introduce a
second toolbar. Keep existing graph columns, row heights, lane geometry and
virtualization; change the surrounding allocation, not graph density.

### Pinned WIP composer

The current WIP row reveals its compose affordance on hover. Clicking it, or a
new compact-only Cmd+Enter (`OpenCommitComposer`) binding in the `!Terminal`
context, opens the pinned composer using the existing commit rendering/action
path. Wide windows and a missing/off-screen WIP anchor propagate the shortcut
without attaching or focusing an undrawn input, or changing the bottom tab.
Hover reveals the action, not an ephemeral editor.
Once open, moving the pointer away or clicking outside **does not dismiss** it.
Escape, its explicit ×, or a successful commit closes the surface. An unavailable,
blocked or failed commit does not impersonate success or discard the draft.

Position against the current WIP row, flip when needed, and clamp the composer to
the available viewport. When the anchor scrolls out of view, hide the surface
without deleting the draft; reopening resumes that same draft. Repositioning,
resize, threshold transitions and surface dismissal are presentation changes,
not input recreation or draft resets. Preserve the existing focus/IME and
commit-plan routing; the compact composer is not a new confirmation-modal slot.

### Changes in the bottom panel

Add Changes to the existing bottom-panel `keyboard_nav::TabList`, retaining its
selection/focus convention and the existing Terminal/Operation Log/Activity
paths. Changes contains **two side-by-side, independently scrolling file lists**:
Unstaged and Staged. Each header exposes List/Tree controls reflecting the **same
existing tree mode**, plus its column's bulk Stage All / Unstage All action. Reuse
existing row selection, per-file stage/unstage controls, path truncation with full
tooltip/AX text, and diff opening.
The composer and Changes are separate surfaces over the same commit state, not
a copied full Commit Panel squeezed into a bottom pane.

### Ownership and safety

There is **one existing `CommitPanelView` entity** owning title/body input entities,
unsent draft, selected file and staging presentation state. Wide panel, compact
composer and compact Changes are surface adapters rendering that owner. Never
clone the draft, selection or staging model into a compact entity; changing
surfaces must not recreate inputs or synchronize two sources of truth.

Compact selection records the valid file reference in that owner's existing
`selected_file` after the original indexed lookup; wide-origin selection is
unchanged. Graph selection may close the desktop visibility flag without closing
the pinned compact composer. Threshold transitions are symmetric: an open wide
panel pins the same owner when entering compact, and a compact pin restores that
owner's desktop flag when entering wide. An undrawn composer input yields focus
to the root, even before kit has painted its first Popover. Retire the old Popover
lease on dismissal; its callbacks cannot erase a new owner's pin.

Repository reads remain session-owned `Reads<TabViewState>` keyed by `SessionId`
under ADR-0183. Compact rendering borrows the active owner's read; it does not add
an active/cache mirror, retain a second read owner, or let a departed/background
read publish into another tab. Existing session lifetimes and draft boundaries
continue to apply.

The safety pipeline remains **plan → confirm → preflight → execute → verify →
oplog**, with existing leases, trust, refusal reasons, refresh and error reporting.
ADR-0093's single `ActiveModal` slot remains unchanged, as does Commit Panel's
separate commit-plan owner. Existing EN/JA message paths name new surfaces and
controls; layout does not bypass confirmation or add direct Git/I/O in render.

Changes is app-global but its panel is session-owned. If a tab/new repository
becomes active with Changes visible and no panel, defer attachment through the
original open path after its read lands; never do repository I/O during render
or display another owner's lists. Until attachment, show the existing localized
loading label. Compact→wide canonicalizes the stored bottom tab to Operation Log
so the rendered tab and status-bar toggle have the same state. Returning compact
with a pinned composer selects Changes again.

Rail SVGs carry their own primary-label colour and non-shrinking size: GPUI's
Svg painter checks the element's explicit colour rather than inheriting it from
the trigger div. The WIP reveal is an inline, non-shrinking action before the
diffstat, not an absolute overlay that can obscure live change counts.

### Integration with the shared keyboard/AX Commit Panel

The later #1251 renderer split is authoritative for dense file rows, the
List/Tree TabList, named ghost footer Buttons and the co-author ListBox.
Compact adapters call those same updated render helpers; they do not retain the
former custom footer as a second implementation. Each Changes column has a
stable roving selector focus owner while sharing the existing tree-mode state.
Composer/input synchronization stays on the same CommitPanelView entity.

The window Escape fallback respects the shared front-layer modal/menu boundary,
then diff-line selection or a PR drawer, then the newest toast, then a compact
rail Popover, then the pinned composer, and finally remaining workspace fallback.
The co-author menu therefore dismisses before the popup and restores its opener
through existing menu reconciliation. Closing the composer yields its hidden
input focus to the root. The lifecycle native oracle exercises menu, error toast,
rail and composer together; no second modal slot or Git write path is introduced.

Warned/blocked compact Commit plans and successfully opened Amend modals request
the existing modal root-focus handoff before suspending a keyboard-focused footer.
A controlled pinned Popover has no implicit prior-focus restore, so leaving focus
on an unmounted Commit or Amend control prevents Escape reaching the visible modal.
Direct execution and wide modal presentation remain unchanged. Native lifecycle
coverage checks the drawn plan heading, modal root focus, Escape returning the same
pinned owner/inputs/draft, next Escape dismissal, and unchanged Git index.
Incoming narrow keyboard fixtures provide real untracked WIP; wide fixtures stay
unchanged. Localization tests close the occluding composer before pointer-selecting
Changes, then reopen the same WIP owner rather than clicking through a pinned popup.

Only the actual picker render owner (desktop Commit Panel or compact composer)
reconciles co-author row focus. Changes prepares shared lists/inputs but never
reconciles a picker it does not draw. Reconciliation from both compact projections
could prematurely treat an End-selected lazy tail as scrolled away before its
layout, despite the selected AX state. The existing 20-candidate keyboard oracle
retains real End focus, tail containment, Enter/Space insertion and opener return;
the shared row navigation algorithm and wide single-reconciliation path are unchanged.

Compact headers retain empty-column Stage all/Unstage all controls, disabled and
keyboard-focusable; desktop headers keep the original disappearing-control focus
fallback. The shared bulk-key oracle follows those actual visibility contracts,
checks Enter/Space cannot mutate the index or repeat an operation when disabled,
and reaches both compact column tab groups with real Tab/Shift+Tab. Empty Unstaged
uses an EN/JA no-changes reason, distinct from the unchanged conflict-only reason.

## State and verification contract

Rest retains neutral theme surfaces; hover reveals the WIP action and rail
interaction; pressed, selected and focus-visible reuse existing Kagi/kit control
contracts. Disabled commit/staging actions retain their localized reasons.
Loading, empty and error presentation reuse existing session/file/composer paths.
There is no new palette, animation policy or repository-operation state machine.

Navigator Tier A `compact_layout_rail` first rejected the original production
source with `missing compact-rail`, then passed the 16-cell EN/JA, Apple
Light/Dark, 100%/167%, 940×660/940×1000 matrix. It also checks Catppuccin and
999→1000→940 logical-width transitions, real filter input, all seven Popovers,
Escape/outside dismissal and leaf selection without a Git mutation.
Changes Tier A `compact_layout_changes` rejected the gated navigator-only source
with `missing compact-tab-changes`, then passed the same 16-cell matrix with
equal column bounds, actual independent wheel scrolling, both List/Tree headers
and original entity/input identity across 940→1392→940. Actual Stage all and
Unstage all update the fixture's Git index; their empty-column controls remain
present and inert. Composer Tier A `compact_layout_popup` rejected the Changes
source with `missing compact-wip-row`, then passed the same 16-cell matrix with
actual hover/reveal, WIP click, ⌘⏎, pointer/outside persistence, Escape/×, scroll-out
draft retention and short-window containment. `compact_layout_geometry` also
passes measured rail/graph/columns/popup containment over the full matrix.
Shared-state Tier A `compact_layout_state` first rejected the original source
with its missing rail. Integration also exposed the previously unset selected
file and a missing wide `cp-commit-action` after Graph selection. It now passes
all 16 cells with actual per-file staging and selection, 940→1392→940 input/entity,
draft/selection/index identity, and a real Commit whose HEAD parent/message/index
are checked while unstaged WIP remains and the popup closes.
Review's six compact scenarios and the extended desktop/co-author oracles pass
the filtered native run (8 passed, 0 failed). Rail additionally proves all seven
glyphs paint foreground strokes without replacing their transparent background.
State includes the wide-first shrink and Graph-selected co-author Escape path;
popup checks an offscreen shortcut leaves the tab/draft untouched.
`compact_layout_lifecycle` checks clean-WIP shortcut/focus, deferred new-repository
and cached-tab Changes ownership/draft retention, real tracked diffstat separation,
and canonical Operation Log selection/first-click toggle in EN/JA, Light/Dark.
The integration lifecycle additionally dispatches Escape from the actual focused
Summary and body inputs, keeping the same draft entity and restoring root focus.
Warned Commit plans and Amend are cancelled through the existing modal root,
without committing or discarding the retained draft. Header-only keyboard
fixtures first dismiss the composer through real Escape; composer and picker
keyboard behaviour remain separate native lifecycle oracles.

Compact headers retain empty-column Stage all/Unstage all as focusable disabled
Buttons with EN/JA reasons. Completing them by pointer returns focus to the
existing navigation root, retaining diff-file arrow routing; keyboard completion
keeps the now-disabled action focused. Wide headers retain their disappearing-action
fallback. After Unstage all restores the left file row, Shift-Tab from the right
header reaches that row's existing Stage SDK Button before the left List tab.
The native oracle measures the actual row Button and compares its perimeter
focus paint before/after Shift-Tab, then requires the named left List focus
handle. It does not remove the row Button's Tab stop or accept an unclassified
focus target. Disabled Enter/Space also preserve the real index and operation
contract counts.

The required compact matrix includes 940×660 and 940×1000, EN/JA, 100%/167%,
Apple Light/Dark and a non-Apple preset; exact-threshold transitions; both toolbar
modes; graph outer bounds and unchanged row/column/lane geometry; real measured
WIP clicks; popup containment/flip/clamp; Escape/×/successful-commit dismissal;
pointer/outside-click persistence; scroll-out and resize draft retention; and
independent Changes scrolling, List/Tree, bulk/per-file staging and diff selection.
The initial compact scenarios rejected their baseline's missing rail/surfaces.
Review's `compact_layout_lifecycle` also rejects the original unguarded shortcut
path with `Cmd+Enter without WIP must not attach/focus an undrawn composer`.
Its guard-removal Before keeps the same native oracle and restores the guard
before After/gates; no repository rows or composer state are substituted.
Existing wide-layout regression oracles remain intact.
The narrow viewport regression adds real untracked WIP solely to provide the new
row anchor; the wide fixture and its recorded composer/file/action bounds stay
unchanged.
The exact-breakpoint desktop viewport adds 1000×660/560 at 167%, including the
480px user-width/right-edge List/Tree matrix, retaining the desktop file/composer
non-overlap and scrolling oracles from #1167. Presentation sizing/publication
lives with the existing Commit Panel surface adapters, keeping workspace.rs
below the 800-line target.

UI-only bulk, co-author and bottom-tab fixtures scope the existing
`KAGI_OFFLINE=1` mode and restore its prior environment value on drop. This
removes unrelated avatar/update/LLM probes from the fixed 120-second native
deadline without dropping any matrix cell or keyboard/pointer operation.
Footer model-picker activation keeps the real online admission contract: global
offline mode intentionally disables generation and cannot prove that oracle.

Final feature-branch integration passes the filtered native suite: **40 passed,
0 failed of 40**, with the same 120-second per-scenario deadline. A focused
footer/co-author/bottom-tab preflight also passes **3 passed, 0 failed of 3**.
The six `compact_layout_*` scenarios, existing Commit Panel/Home/overflow/bottom
panel oracles and worktree/diff regressions all remain enabled. Functional gates
pass: locked build, Kagi tests (773 passed, 5 ignored), UI-core tests (197 passed,
8 ignored), workspace clippy with existing warnings only, formatting and all 22
CI invariant gates. The final command is a plain production `cargo build` after
the native-feature run.

PM's initial Tier B at 940×660, Apple Light, Icon and Text found invisible rail
glyphs and an overlapping Commit reveal/diffstat. These are real-window findings,
not dismissible because the preceding bounds-only oracles passed. Review adds
native foreground/background SVG pixels and reveal/stat non-intersection.
**PM Tier B of the revised window is pending**: compare before/reference/after
with the same data, inspect EN/JA/high zoom, and check focus, IME/accessibility.
Bounds/pixel probes and prototype approval do not establish those manual results.
Build/unit/clippy/invariant results belong to the functional handoff report.

## Amendment — development cutover (2026-10-13)

The owner approved integrating the feature into development. The restored
document titlebar above the existing toolbar is authoritative in both compact
and wide windows; the compact breakpoint and rail width remain unchanged.
Shared Changes and desktop file lists use the development branch's 22-reference-px
slots (20 logical px minimum), typography, header spacing and inset action focus
rings. Their viewport floors account for the same slot plus border allowance.
Selected WIP/file labels and diffstat use the shared on-accent foreground guard.
The shared composer retains development's input/action minimum height and
draft-save failure/retry feedback, rather than keeping pre-titlebar sizing in
the compact-only adapter. ADR-0048's English Git terms apply to compact Japanese
messages as well as the existing desktop messages.

The density oracle distinguishes desktop's single-line header from Changes'
two-line heading/control header, retaining literal 1px padding, its scaled
`gap_1`, selector border and control containment. Native focus-ring pixels are
sampled after clicking the pinned composer's real close button: at 167% its
shadow reaches Changes, so a covered ring is not the file-row paint itself.
The exact accent-pixel and 22px slot expectations remain intact.

Development-cutover Tier A passes **36 passed, 0 failed of 36**, filtered to
`compact,rail,sheet,titlebar_tabs,commit_panel_,commit_row_layout,selection_on_accent,sidebar_rows,pane_budget_`,
with KEEP_GOING and the unchanged 120-second per-scenario deadline. This includes
all nine `compact_layout_*` entries (the geometry matrix remains partitioned),
both restored-titlebar scenarios, density/on-accent/sidebar/pane-budget neighbors,
and shared composer/file-row keyboard, localization, reload and selection checks.

Real-window Tier B now records Apple Light/Dark at EN/100%, dynamically resizing
the production build from 1392×883 to 900×660 with the same staged/unstaged fixture.
Window-only captures show the wide navigator/right Commit Panel, compact pinned
composer plus Changes columns, and the local-branch rail Popover plus Changes.
Evidence is external to Git at
`/tmp/kagi-hig-audit-20261008/compact-layout-tierb/`. Each app uses the isolated
`kagi-compact` USER/log directory and copied stress repository; only its recorded
PID is briefly activated for capture, the previous foreground is restored, and
each process is quit afterward. This is screenshot/click/resize evidence, not
JA/high-zoom/IME/VoiceOver manual acceptance.

Review removes the unused compact-only list/selector/bulk-action message keys:
Changes renders the original `CommitPanel*` localization tested by the shared
Commit Panel chrome test, not a parallel English-only message family.

## Amendment — 2026-10-14: compact commit Inspector and bottom-panel height (#1484)

[#1484](https://github.com/TomiXRM/kagi/issues/1484) extends the existing Changes
tab rather than adding another Inspector tab or selection owner. Below the
unchanged **1000 logical px** breakpoint, a selected non-WIP commit presents
two side-by-side columns: **Inspector metadata on the left**, **changed files
on the right**. Metadata reuses title/message, author, date, hash, ref chips,
Branch here and Cherry-pick rendering and actions. Files reuse the normal
Inspector's Path/Tree controls, RowList, stable-path selection, keyboard reveal,
accessibility and dense 20-reference-px rows / 13-reference-px typography.
Selecting a file opens the existing central diff. Loading, empty and error
states continue to come from the existing Inspector/session read paths.

The historical-commit adapter borrows the same session-owned read, Inspector
state and action routing as the wide right pane; it does not clone the read
model or create a second file-selection/focus owner. Selecting WIP restores the
existing Unstaged/Staged Changes columns, retaining their original CommitPanelView
and draft/staging ownership. Keep the existing localized Changes tab name and
EN/JA Inspector labels and actions; no parallel message family is required.
At or above the breakpoint, restore the existing right Inspector.
The compact metadata column scrolls its complete existing header and message
together; do not squeeze or clip the Branch here/Cherry-pick actions at high
zoom to make the short panel appear to fit. The two column viewports retain
equal allocation, with the shared system scrollbar policy and file RowList.

Review amendment: Changes uses the existing workspace projection precedence,
**Compare before Inspector**. Compare with HEAD/working tree and stash/PR peeks
render their own synchronized file snapshot and comparison title; file activation
must resolve the exact file from that same snapshot, never a commit-file index
against a comparison owner.

Explicit WIP activation retains the existing CommitPanel-before-Compare
precedence: its active staging owner draws Unstaged/Staged even if a suspended
Compare entity is retained. Compare-before-Inspector applies to the read surface
when that WIP gate is closed or a historical commit is selected.

Entering compact saves the desktop bottom tab and open/closed presentation in
window-local transient state; widening restores both along with the preferred
height. Selecting a normal commit updates content only when Changes is already
being shown. It does not open a closed panel or replace Terminal, Activity or
Operation Log. The existing explicit WIP/composer activation may select Changes
inside compact, but cannot overwrite the saved desktop presentation. This
supersedes the earlier Operation Log canonicalization on compact exit.

Native Graph pointer-focus coverage must target the drawn navigator: at compact
widths open the rail Popover before its filter; desktop Cmd-F/filter coverage
uses widths at or above 1000px. Pointer selection then restores Graph focus
without changing filter text. Copy-receipt assertions measure a local boundary
inside the bounded contract-log tail rather than treating that tail as a
monotonically growing event count.

The compact bottom panel defaults to **one-third of the logical window
height**, measured for the **entire panel including its tab strip**, not just
the Changes body. Apply this as presentation-only fitting, following
[#1355's preferred-versus-fitted policy](0217-graph-sidebar-panes.md#amendment--2026-10-12-horizontal-pane-budget-1355).
Keep the saved preferred bottom-panel height authoritative for the wide layout:
entering compact, fitting on window/zoom changes, and compact divider drags must
not write a compact height to settings or replace the wide preference.
The divider remains interactive using the rendered geometry; compact drag
adjustment is transient presentation state, not a second saved preference.
Widening restores the normal preferred height. Continue to respect existing
viewport containment and minimum-content budgets when fitting a small window.

Acceptance remains a verification contract, not a claim of results here:
Tier A at 900×660 measures metadata/files columns, actual file-click diff opening,
WIP's Unstaged/Staged switch, and total bottom-panel height near 220 logical px
(within one file row). It exercises divider adjustment and 900→1392 restoration
of the right Inspector and preferred height, and proves settings retain the
wide height. Cover EN/JA × 100%/167% × Apple Light/Dark. Tier B captures the real
900×660 commit and WIP selections in Apple Light and Dark. Existing wide
Inspector keyboard/AX and WIP composer/state contracts remain authoritative.

## Consequences

Narrow windows gain graph space without changing toolbar or graph geometry.
Commit state survives relocation because ownership does not move or duplicate.
The compact surfaces add placement and dismissal responsibilities, but no second
commit workflow, new read store, persisted layout mode, or wide-layout redesign.

## Amendment — 2026-10-14: shared Inspector segments and bulk hover (#1495, #1496)

Historical Changes and the wide Inspector use the same session-owned roving
`keyboard_nav::TabList` for Path/Tree. Arrows select automatically; Enter/Space
explicitly set the focused mode, including reactivation of the selected mode.
Expose two named, selected Tabs and one Tab stop, preserving the neutral
`segment_selected_bg()` fill and existing selected-file/diff ownership. Hidden
Inspector controls yield focus through the existing root fallback.

Bulk Stage all / Unstage all / Discard all retain their success/warning/danger
captions on neutral `surface` hover. These secondary operation controls do not
borrow opaque active-row accent fill, so no on-accent replacement is necessary.
Disabled controls never acquire accent hover or execute keyboard activation.
Existing confirmation, staging and repository ownership are unchanged.
