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

The current WIP row reveals its compose affordance on hover; clicking it, or the
existing Cmd+Enter command, opens the pinned composer using the existing commit
rendering and action path. Hover reveals the action, not an ephemeral editor.
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
Unstaged and Staged. Each header retains its own existing List/Tree control and
bulk Stage All / Unstage All action. Reuse existing row selection, per-file
stage/unstage controls, path truncation with full tooltip/AX text, and diff opening.
The composer and Changes are separate surfaces over the same commit state, not
a copied full Commit Panel squeezed into a bottom pane.

### Ownership and safety

There is **one existing `CommitPanelView` entity** owning title/body input entities,
unsent draft, selected file and staging presentation state. Wide panel, compact
composer and compact Changes are surface adapters rendering that owner. Never
clone the draft, selection or staging model into a compact entity; changing
surfaces must not recreate inputs or synchronize two sources of truth.

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
Shared-state/commit-success gates are recorded with their functional commit;
their result is not inferred from the preceding surface PASS results.
The required compact matrix includes 940×660 and 940×1000, EN/JA, 100%/167%,
Apple Light/Dark and a non-Apple preset; exact-threshold transitions; both toolbar
modes; graph outer bounds and unchanged row/column/lane geometry; real measured
WIP clicks; popup containment/flip/clamp; Escape/×/successful-commit dismissal;
pointer/outside-click persistence; scroll-out and resize draft retention; and
independent Changes scrolling, List/Tree, bulk/per-file staging and diff selection.
New compact scenarios must first reject the baseline for its missing compact rail,
then execute their complete behavior assertions after production integration.
Existing wide-layout regression oracles remain intact.

**PM Tier B is pending**: compare before/reference/after in the real window using
the same data and viewport, inspect narrow EN/JA/high-zoom states, drive pointer
and keyboard reveal/dismissal, and check actual focus, IME and accessibility.
Bounds probes and the user's prototype approval do not establish those results.
Build/unit/clippy/invariant gate results belong to the slice's handoff report;
the navigator PASS does not establish the other compact surfaces or PM Tier B.

## Consequences

Narrow windows gain graph space without changing toolbar or graph geometry.
Commit state survives relocation because ownership does not move or duplicate.
The compact surfaces add placement and dismissal responsibilities, but no second
commit workflow, new read store, persisted layout mode, or wide-layout redesign.
