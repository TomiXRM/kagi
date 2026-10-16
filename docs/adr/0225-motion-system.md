# ADR-0225: Shared view-owned motion

- Status: Accepted
- Date: 2026-10-16
- Issue: [#1617](https://github.com/TomiXRM/kagi/issues/1617)
- Extends: [ADR-0173](0173-reduce-motion.md)

## Context

The owner requests selection crossfade, list insertion and animation of existing
accordions. Pane transitions already own reversible cubic timing, a frozen GUI
clock and Reduce Motion policy in `src/ui/panel_motion.rs`. Adding separate
clocks or relying on component-specific animation would duplicate that policy.
This foundation moves the existing owner into `kagi_ui_core::motion`; pane
layout, toggle interpretation and retained Inspector content remain with
`PanelMotions`. No new selection, insertion or accordion surface is wired here.

## Decisions

1. **No fork or dependency change.** Use public GPUI APIs shared by the pinned
   Zed `caf5007` and crates.io `gpui-pre 0.3.9`; do not require the latter's spring
   or built-in Reduce Motion APIs. `kagi-domain` remains pure.
2. **One clock and policy.** `motion::now()` uses the monotonic clock in normal
   builds. Under `gui-e2e`, `set_clock_for_e2e(Some(Instant))` freezes that same
   clock for every consumer; `None` restores the default-instant runner policy.
   `motion::instant()` also consults `theme::reduce_motion()`, the effective
   macOS-system OR app preference. Read policy on every render, not only on
   transition start, so enabling Reduce Motion settles in-flight state.
3. **View-owned frames.** Sync retained motion from the owning view, read its
   clock-driven progress and call `motion::request_frame(window, moving)` there.
   It requests no frame for settled, reduced or frozen test motion. A frozen
   scenario advances the clock and explicitly draws. The ecosystem renderer
   documents that `with_animation` stalled inside `overflow_y_scroll`.
   Source inspection confirms pinned `AnimationElement` advances and schedules
   only in `request_layout`; it does not establish a universal upstream ban on
   scroll-contained animation. Our scheduling deliberately does not depend on
   a virtualized or cached scrolling child's layout being revisited.
4. **Timing and interruption.** Use 150–250ms one-shots. Existing pane opening
   stays 180ms cubic ease-out; closing stays 150ms cubic ease-in, with duration
   scaled by remaining distance. The default shared transition is 180ms.
   Retarget from the currently drawn scalar, never its old endpoint. A keyed
   transition increments its generation on key/target changes and reports
   bounded value, elapsed progress and completion; unchanged targets do not
   restart. First-render reveals settle immediately.
5. **Fixed-height row splices use approach A.** Keep `uniform_list`; publish the
   final list length immediately. Each inserted key contributes
   `-row_height * (1 - presence)` to rows below it and fades in. Each removed key
   contributes `+row_height * presence` to rows below it and fades out as retained
   ghost content. N simultaneous insertions/removals therefore displace by
   `-N*h*(1-e)` / `+N*h*(1-e)`, respectively. All offsets settle at zero.
   Overlapping batches leave unaffected keys running; reversing a key retargets
   its current presence. Switching its final-list membership changes the offset
   baseline by exactly one row, preserving its painted position.
   The caller supplies logical order including ghosts and the zoomed row height.
   This is presentation displacement, not row-height animation or list migration.
   **Large changes are instant:** `MAX_ANIMATED_ROWS = 64` bounds simultaneously
   retained keys to approximately one screen. If a batch would exceed that cap,
   settle the entire owner (including earlier active keys) immediately; do not
   animate a partial subset of a large fetch or directory collapse. Row sampling
   and retirement therefore visit at most 64 entries per call, regardless of
   total list size. Duplicate/reversed keys already within the cap still animate.
6. **Identity is logical, never positional.** Retain row presentation by
   `CommitId`, worktree path or file path, scoped to the owning session/surface.
   List indices may locate geometry but are never animation identity. Remove
   settled splice state and state for removed/replaced owners. Selection
   intent and accessibility state remain immediate and authoritative; animation
   changes only the visual presentation.
7. **Measured reveal.** `Reveal` exposes fraction, clipped height and
   `retain_content`; keep closing content mounted until settled. Measure the
   natural content, never the clipped height. `motion::clip` preserves full-size
   content and changes only its outer box. Do not use variable row-height reveal
   inside `uniform_list`. The accordion slice must compose around that invariant.
   Adding disclosure to currently non-collapsible Commit Panel folder headers or
   Settings groups is outside #1617.

## Shared API and ownership

- `KeyedTransition<K>`: `new(key, value)` creates settled state;
  `retarget(key, target, now, duration, easing, instant)` changes the generation
  and continuously retargets a normalized scalar. `sample(now)` returns
  `TransitionSample { value, progress, done }`. Crossfade can retain a transition
  per logical row, using selected/unselected as its target key and 1/0 as opacity.
  This supports A → B → C and reversals without dropping still-visible fades.
- `RowSplice<K: Eq + Hash>` replaces the single-insertion helper. Construct with
  `default()` per list owner. On each logical change call
  `splice(inserted, removed, now, instant) -> bool` once with both sides of the
  diff; never split a replacement into two calls. Do not call it on viewport
  mounts or each render. The return value is true when instant policy or the
  combined key limit settled the entire owner, false otherwise.
  Before touching any transition, stage the distinct batch keys and check the
  union of existing active keys and unregistered batch keys against
  `MAX_ANIMATED_ROWS`. Overflow clears all active motion and stops consuming the
  batch; the caller has already published the complete final list. No insertion
  or removal subset starts animating before this decision. Thus insertion-heavy
  and removal-heavy replacements, and either iterator order, follow one policy.
  Existing keys reverse from their current value; unaffected keys continue.
  Duplicate keys count once; a key supplied on both sides resolves to removal.
  Temporary staging is event-only and bounded to 64 distinct keys, not per frame.
  Call `advance(now, instant)` each render to retire settled keys or clear all
  motion under instant policy; its bool is the owning view's frame demand.
  `row(key, row_height, now, precedes)` returns `RowSample { offset, opacity }`.
  The callback identifies active keys before that row in the caller's logical
  order, including retained removals. Cache ordering when the list changes;
  do not scan the full list per callback. Only active keys are stored.
- For removal, retain the real removed content and its logical order in the
  view until `ghost(key, after, between, row_height, now)` returns `None`.
  `after` is the nearest surviving predecessor key (or `None` for list start);
  `between` supplies distinct active removed keys between that anchor and this
  ghost, at most `MAX_ANIMATED_ROWS` entries—not the historical removed set.
  The implementation also caps iterator consumption at that bound.
  `GhostSample { after, offset, opacity }` locates the ghost relative to the
  anchor's animated top, or to list start. Its offset sums intervening removed
  keys' current presence plus the anchor's own current presence (normally one
  row, but partial when that anchor was just inserted/reinserted). Recompute
  anchors after overlapping splices; neither key nor position is copied during
  sampling. Ghosts are presentation-only, not additional selectable list rows.
  A reinserted key stops being a ghost immediately and continues fading as its
  normal row with unchanged opacity.
- `Reveal`: `sync(open, now, instant)` before `sample(natural_height, now)`;
  preserve existing open-state ownership. `visible` and `animating` also support
  the existing pane clipping integration.
- `Axis`, `Anchor`, `clip`: unchanged pane clipping implementation, now shared.

These are view presentation primitives, not another state store, asynchronous
animation service or domain model. Samples do not allocate; key ownership and
active collection lifetimes belong to the caller.

## Validation boundary

Ordinary math tests cover key generations, interruption, exact pane endpoints,
batched insertion/removal, overlapping splices, reversals, ghost reanchoring,
instant settlement, measured heights and closing retention. Existing
native motion scenarios migrate their frozen-clock calls without changing their
behavior coverage. A separate oracle repair allows the existing 1px tolerance
for five settled-width checks: unchanged dev reproduced the exact comparison
of pixel-snapped 327.5px against unsnapped 327.6px. Native Tier A remains the
behavior gate. Math tests and hidden native windows do not establish Tier B
visual comparison; completed real-window evidence is recorded per surface.

## Changes, Inspector and operation-disclosure amendment (#1617)

Changes keeps presentation on `CommitPanelView`, scoped separately to each staged
side. Stable file paths identify selection and splice transitions; cached logical
ordering is rebuilt with status/tree/fold changes, never on animation frames.
`uniform_list` publishes its final row count immediately. Only visible live row
content and a bounded set of retained, noninteractive removal ghosts are sampled.
Initial loads, display-mode changes and status changes larger than the viewport
settle immediately. Generated and Agent-artifacts child rows use the same splice
path, so closing retains presentation without retaining selectable children.

Displaced edge rows outside `uniform_list`'s final-slot viewport are mounted once
as out-of-flow children of its first visible slot, with real final-index input
and AX identity. Overflow expansion is bounded by the shared active-splice cap.
Motion offsets and ghost placement use the list's measured pixel-snapped pitch,
not the configured height. Reopening loading placeholders and their first real
status publication settle without replaying a previous visit.

Inspector extends its existing session-owned file-row presentation rather than
creating another selection model. Its single historical/compare file list uses
stable path identity; Generated fold children and Tree directory descendants use
fixed-height splice motion.
Selection intent, focus reconciliation, AX selection and chevrons remain
authoritative immediately, independently of the painted selection wash.
On-accent filename, status and diffstat foregrounds follow the same wash progress
as the background; logical selection still changes immediately. Inactive-window
foreground policy remains unchanged.

Supporting modal sections and equivalent/recovery commands share measured
`Reveal` clipping and retain only drawing while closing: Note semantics, identified
input containers and scroll handlers disappear with logical intent. Existing
scroll displacement survives visually and resumes on reversal. Presentation lifetime
is reset with each modal generation, preventing content/state leaking into a new
confirmation. `modal_list_panel` is unchanged: destructive-operation targets are
never hidden behind disclosure.

All surfaces use the shared clock, cubic timing, Reduce Motion policy and
view-owned frame demand. Frozen-clock native regressions distinguish intermediate
presentation, continuous reversal and settled/instant endpoints. Native Tier A
does not substitute for isolated, PID-targeted Tier B visual review.

The unchanged `commit_panel_file_middle_ellipsis` scenario also reproduced its
missing painted tooltip on base `9d15fce6`: its hidden window had not adopted
the existing activation-input seam after #1591. Its isolated oracle repair
marks that window active for tooltip lifetime only; the full-path and painted
tooltip assertions and production cancellation behavior remain unchanged.


### In-flight file-row contrast correction

Actual key-window Tier B showed that linearly interpolating normal and on-accent
ink creates unreadable gray glyphs on the partially composited blue wash.
Changes and Inspector therefore keep their exact normal and selected endpoint
styles, but choose one opaque primary/status/diffstat ink during the transition
using the existing on-accent 3:1 policy against the composited row background.
This is a presentation-only colour decision; the wash and semantic selection
continue unchanged. Diffstat bars keep their existing Git encoding.

The native frozen-clock regression samples rasterized glyph cores and real
row-fill pixels around the half-wash frame; renderer colour metadata and an echo
of the interpolation formula are not contrast evidence. Both old surfaces
failed that oracle at 1.943:1 before this correction. The original active-window
frame38 contact sheet is retained as the visual before-failure.

### Inspector generated viewport realization

Generated expansion retains the final fixed-height list count immediately, but
realizes the bounded neighbouring rows whose displaced content intersects the
requested viewport. Inspector mounts those rows once as out-of-flow children
at the appropriate requested edge, preserving logical paint/hit order, real
indices, input handlers and accessibility identity. Placement uses the list's
measured pixel-snapped pitch, not the unsnapped configured height. Closing
ghosts remain presentation-only and the shared splice cap bounds extra work.

Changes uses the same leading-before-first/trailing-after-last placement order.
Both lists give every real row kind a role-less parent boundary, including fold
headers, directories and overflow notes. It consumes clicks after
the row's own child actions, so compressed overlapping rows cannot both activate.
Left mouse-down retains propagation. Changes resets a navigation-owner-local
row claim in list capture, distinct from GPUI default prevention: an inline
Stage/Unstage Button prevents its own focus, not the existing file-row focus.
The first real row claims the event; covered rows cannot take keyboard origin.
An otherwise nonfocusable winning row retains its child's focus policy or uses
the already-supplied ancestor root target. Inspector retains its existing
default-focus convention.
The parent consumes right-button presses only after a child's own context-menu
handler has had its turn, preventing headers/More from opening a covered file's
menu while preserving real-file menus. No broad hitbox occlusion masks ancestor
folder drops, hover, drag, wheel input or the existing child accessibility roles.
Native overlap controls assert actual focus handles, keyboard and right-menu
path identity, non-file ancestor focus, unchanged selection and real folder drops.
Inline Stage/Unstage controls additionally assert actual pointer-down focus,
the rendered AX row role/name, subsequent keyboard-menu/arrow target identity,
and real Git index changes using complete pointer gestures.
Presentation-only removal ghosts never acquire actions or accessibility rows.

Changes removal overlays paint after live rows. Their visible exit-row host
consumes only right-button context presses, preventing a covered live file's
menu from opening; left input, focus, accessibility and ancestor drops remain
unchanged. Native top-file removal coverage proves actual ghost filename ink,
paint order and the clipped intersection before testing the menu no-op and a real
repository-folder drop at that same point. Inspector generated ghosts follow
the normal file rows and live fold header; the native topmost exit-only-area
control remains a no-op without adding a ghost handler.

External-drop controls establish the pointer-modality precondition with MouseMove
after keyboard dismissal, verify a freshly painted closed-menu surface and the
actual visible target, and require the repository to open. GPUI classifies
FileDrop before converting it into mouse events, so Entered alone retains the
preceding keyboard modality. This harness precondition is not an OS-level
keyboard-to-Finder drag claim.

The root's existing actual-center visibility edge retires retained Changes and
Inspector presentation for the departing session on center/owner changes,
without changing logical rows, selection or scroll. The sole continuity exception
is Graph ↔ Diff inside the same owner: opening a file's central diff must preserve
its already-visible file-row fade, as the native first-file selection test checks.
The checked departing-owner lookup also permits last-tab → Home after that owner
has been removed. Returning inside the old transition duration presents settled
endpoints; unchanged hidden frames neither notify nor rebuild file models.
Frozen-clock native regressions leave at a genuine 37ms displacement/wash/closing
ghost and return at the same instant; the existing startup and last-tab Home
scenarios remain unchanged.

## Sidebar and editor tree amendment (#1617)

The dense Repository Navigator and Editor file tree retain their existing row
heights, virtual lists, focus owners and accessibility models. `SidebarState`
owns sidebar presentation; `EditorWorkspaceView` owns Editor presentation.
There is no second tree model or Git read in the animation path.

- Editor selection intent changes immediately. The Navigator has no independent
  selection: its existing keyboard `RowFocus` ring crossfades, without adding a
  persistent selected-looking row fill. Pointer hover keeps its existing
  appearance. Stable ref names, worktree paths, stash commit identities and
  Editor paths key presentation only; no graph-target-driven Navigator wash or
  new AX selection state is introduced.
- Local/remote subgroups and Editor directories use `RowSplice`: final logical
  membership is immediate, children fade/slide, and content below them settles
  into its final slot. Removed content is retained only as noninteractive,
  accessibility-hidden presentation until the closing splice settles.
- Top-level Navigator headers remain mounted while their bodies reveal/close.
  Pane weights and persisted collapse intent retain their existing ownership.
- Chevrons remain immediate rather than crossfading or rotating: they report
  current disclosure intent even during reversal; motion is on the content.
- Sidebar refresh arrivals animate only within the same owner/filter/list
  context. Initial loading, repository changes and large replacements settle
  immediately. Ordinary GUI E2E remains instant; frozen-clock scenarios opt
  into real intermediate presentation.
- Reduce Motion is sampled during rendering so enabling it during a transition
  settles presentation immediately. Animation changes neither selection nor
  the keyboard and accessibility models.

Native Tier A is the behavior gate for mid-flight geometry/opacity, endpoints,
reversal and instant policy. Tier B uses an isolated PID-targeted fixture window;
foreground activation is allowed only with owner authorization. Neither lane
controls the user's pointer; hidden native assertions are not screenshot evidence.

### Review corrections

The Navigator's prior presentation was a keyboard focus ring and pointer hover,
not a selected-row fill. Its keyed transition therefore scales only the existing
focus-ring color, retaining the direct-focus and keyboard-visible gates. Editor
selection foregrounds retain the exact original normal/on-accent endpoints.
During the transition, on-accent becomes eligible once it meets the existing
3:1 policy against the actual composited wash; logical selection remains immediate.

Both virtual trees paint a bounded inert fringe when displaced rows outside the
final virtual range enter the viewport. Motion offsets, ghost anchors and fringe
origins use the public list handle's measured content height divided by its
current item count; the intrinsic slot height remains theme-derived so zoom
changes are remeasured. This avoids cumulative device-pixel rounding drift.
Sidebar initial-read readiness is explicit: loading headers do not consume the
instant initialization boundary before the first accepted asynchronous read.

The whole-Navigator focus boundary also owns its ring lifetime. When the panes
leave the front workspace (hidden sidebar, PRs, Issues or Editor), clear both the
retained focused identity and all ring transitions before yielding logical focus.
Virtual-list processors do not run while away, so deferring that reset until a
row is painted would replay an obsolete ring on the first returning frame.
Frozen-clock native coverage checks immediate focus evacuation and unfocused
Metal ring pixels on return through all four paths (PR #1621 review).

The same lifetime rule applies inside the compact rail: skip-reset every Git
pane that is not the visible popup, and clear all Git rings when the popup is
PRs or Issues. The rail itself remaining front does not keep its replaced
pane's focus presentation alive. Native coverage returns Local → Remote → Local
and Local → PRs/Issues → Local without replaying the skipped pane's ring.

Pane reveal targets use the measured `sidebar-panes` viewport, minus actual
inter-pane divider gaps, rather than the sum of currently painted pane heights.
When all five panes are collapsed, those heights cover only their headers and
omit the viewport's unused flex space. Measuring that existing flex container
keeps the next body's natural reveal height nonzero and current after a closed
window resize. Native frozen-clock coverage reopens from the all-closed state
at three viewport heights, checks real intermediate body height and endpoint
continuity, and retains immediate Reduce Motion opening/closing.

Editor ghosts and displaced edge content also consume right-button presses.
Removing their file/directory handlers alone still lets the blank-tree ancestor
open its root creation menu. The shared presentation wrapper stops that bubble
without restoring any row action or accessibility node. Frozen-clock native
coverage right-clicks an unobscured closing ghost in both tree sources and
retains ordinary file, directory and genuine blank-space menu targets.

Retained Editor motion is retired at the actual resolved workspace visibility
edge, before root rendering can return early for Home or the conflict body.
The root shares its existing center resolution with this lifetime hook; it does
not duplicate route policy or infer visibility from the presence of an entity.
One cached `(SessionId, CenterPane)` is presentation-only: unchanged frames do
no owner scan, entity clone/update or allocation. A newly hidden Editor is
settled once through its existing API, using an optional owner lookup so closing
the last tab is safe. PRs, Issues, File History, Analyze, branch cleanup, loading,
Home and conflict replacement all reach that boundary. Tab-entry retirement
remains, and the still-mounted Editor's tree-only hide already samples motion
instantly through `show_tree`. Frozen native coverage returns from PRs/Issues
at the same clock tick during selection and collapse, checks first-frame settled
presentation in both sources, and verifies ordinary new motion after return.

### Pointer ownership during row compression (#1621)

Translated real rows can overlap their siblings while fixed list slots already
have their final positions. Consuming only the visible paint is insufficient:
GPUI can dispatch the same click to the covered sibling and replace the first
callback's selection. Editor file and directory callbacks consume their existing
Click event. The Navigator claims Click and otherwise-unhandled right-button
presses at its existing translated real-content boundary, after child callbacks.
This includes Tag/Stash/Worktree and no-action Remote/Tag rows, not just branches.
It does not stop left MouseDown, introduce generic occlusion, or change the
existing `under_ring` focus/default-prevention route.

Compact pointer worktree selection and successful-leaf dismissal move with that
same real boundary; fixed-slot keyboard activation/dismissal remains unchanged.
Disabled rows keep their existing feedback without dismissing the popup. Child
localhost links consume first, open exactly their own URL, and neither select a
repository nor dismiss the pane. The native-test URL recorder follows the
existing isolated-link convention; ordinary builds still dispatch the browser.
Motion and focus remain keyed by their existing semantic identities.

A retained top-removal ghost paints after its real anchor inside the slot, so a
handler-free ghost can still expose the covered row's context menu. Claim only
Right MouseDown on that ghost with a normal nonoccluding hitbox. Left remains
transparent, no keyboard/AX row is restored, and ancestor external-path drop
remains reachable in the normal Navigator. Viewport fringes retain inert
rendering: native geometry establishes their visible clip-edge pixels do not
overlap later real rows, rather than adding a speculative event blocker.

Frozen-clock native input at 37ms verifies actual selection/peek/repository and
context-menu targets, focus, disabled-target feedback, compact dismissal and
child links in normal and compact panes. Separate top-removal and clipped-fringe
controls exercise right/left/drop dispatch, not just hit rectangles; Editor
coverage uses both Changes and All files, including File-over-File and
Directory-over-File compression. Existing keyboard and external-drop scenarios
remain controls.

## Graph surface amendment (#1617)

The dense Graph retains its existing stripes, lane tints, fixed row height and
selection endpoints. Its single presentation owner lives in `TabUiState`, not
the published read model. Selection transitions are keyed by commit OID, stash
OID or worktree path and retain all still-visible outgoing washes through
A → B → C retargets; keyboard/AX selection remains immediate.

Only accepted full reads authorize commit arrivals. Initial/activation reads,
pagination, filter/solo changes and broad rewrites settle immediately. Live WIP
prefix membership is compared separately so cheap status refreshes do not need
to rebuild commit rows. Changes larger than the measured viewport settle.
Removed WIP rows retain their old content as presentation-only ghosts.
Reset again when an initial/activation read is accepted, not only when it
starts: a cached pre-read frame can consume the earlier reset. This also
excludes the first dirty unborn snapshot while preserving later live WIP motion.

The uniform list adopts its final length immediately. Fixed-height slot shells
offset/fade their complete row content, including graph canvases: `graph_view`
paints from the canvas's measured absolute bounds, so lanes move with their
nodes and return to the original connector geometry at rest. The viewport
clips overflowing presentation rather than changing row heights.
Because the pinned public `uniform_list` realizes only final-index visible rows,
expand the presentation range only for edge content displaced into the viewport,
including visible ghosts whose anchor scrolled out. Mount these real rows once as
out-of-flow children before the first and after the last realized slot, preserving
logical paint order and retaining their true final index for hit testing and AX.
Expansion is capped at `MAX_ANIMATED_ROWS` additional
slots; fixed list length and geometry never change to accommodate animation.

When a splice occurs above a scrolled viewport, preserve the first visible
logical key and its within-row pixel offset, and settle that splice instead of
animating the reading anchor. At the top, new arrivals push visible content.
Use the measured list pitch (`last_item_size.contents.height / item_count`)
for offsets and anchor arithmetic, while keeping intrinsic slot heights
theme-scaled so zoom can remeasure them. Compare scroll offsets numerically:
GPUI's total pixel ordering distinguishes negative zero from positive zero,
but a programmatic scroll to negative zero is still the unanchored top.
Cache identity/order only on changed reads/prefixes; frame sampling visits only
visible rows and the bounded active/ghost set, never rebuilds the read model.

Native frozen-clock coverage checks midpoint wash/geometry, interruption,
settlement and Reduce Motion. Existing default-instant scenarios retain their
oracles. The empty/unborn-list case retains its final disappearing WIP ghost
over the empty-state page until settlement. Tier B uses an isolated,
PID-targeted application; foreground access is owner-authorized but serialized
with the other motion slices. The owner's screen is not locked.

### Graph overlap hit ownership

New rows in a small multi-commit batch temporarily have less than one full row
of visual spacing. GPUI's normal hitboxes can dispatch the same click to each
overlapping row, letting an older handler overwrite the visible row's selection.
Real commit, stash and WIP click handlers therefore stop propagation after
performing their existing action. Hitboxes remain normal: ancestor hover,
external folder drops and mouse-down focus still work, as does wheel chaining.
Ghosts and fixed slot shells remain inert; only the winning real click is consumed.
Leading fringe paints before the first requested row and trailing fringe after
the last, matching logical row order rather than mounting both at the first slot.
Native coverage clicks both the actual displaced tail and the visible strip of
a compressed new row at 37ms, at 100% and 167% zoom, and compares CommitId.
Native controls also drop external folders onto occupied commit/WIP/stash rows
and click a stash from a focused sidebar Input, requiring Graph keyboard focus.

A closing first-prefix WIP ghost paints after the surviving commit and can cover
its context-menu hitbox. Consume only Right MouseDown on that inert ghost, so
the covered commit cannot open its menu or become selected. Do not occlude
other input: a real external folder drop through the same ghost still reaches
the ancestor. The genuinely visible leading strip of an arriving current WIP
has no underlying row and needs no context handler. At 37ms and both zooms,
native right-button controls exercise these negative targets and the actual
commit, stash and linked-worktree menu owners in compressed geometry.

### Graph retained visibility boundary

Graph joins the root's existing actual-center visibility edge, not a second
workspace-mode routing table. Leaving a visible CommitList marks that session's
existing presentation reset flag. Only an actually visible Graph CommitList
consumes selection/splice synchronization: hidden frames neither schedule Graph
motion nor repeatedly rebuild its identity caches.

The reset survives full reads and cheap WIP updates while hidden, so the first
returning frame settles old washes, displacement and retained ghosts, including
newly accepted hidden arrivals. It then clears normally; later visible arrivals
still animate. The root's cached owner/center edge also covers Home, conflict
takeovers and repository ownership changes without scanning sessions per frame.

`graph_mode_motion_reset` starts with real partial wash/displacement at 37ms,
hides Graph behind PRs, Issues or Editor, accepts a real commit read while hidden,
and returns without advancing the clock. It requires no ghost and exact endpoint
wash/geometry on the first visible frame, then proves fresh WIP motion still runs.

## Commit changed-file directory disclosure (#1628)

The existing session-owned `InspectorModel::keyboard` also owns directory
disclosure; wide Inspector and compact Changes use the same renderer. Directories
start expanded. A collapsed-key set uses full repository-relative paths, including
compressed chains, rather than row indices or directory basenames. Nested intent
survives ancestor toggles and Path/Tree switches. Changing the selected commit or
compare source resets it; refreshing the same commit's derived file data does not.
An unavailable intermediate read is not evidence that a directory disappeared:
prune absent keys only from an available tree, preserving folds across that gap.

Directory rows join the existing RowList: pointer and Enter/Space toggle,
Left closes, Right opens. Their TreeItem expanded state and visible keyboard
membership change immediately. Collapsing a directory never replaces the open
file diff. Generated disclosure remains independent.

The existing Inspector splice now compares all visible logical entry keys,
including directories, against one canonical full-tree order. Hidden descendants
remain in that order but not in the interactive list. Removed file and directory
content is retained only as inert paint, attached to its nearest surviving
predecessor and displaced with that anchor. No second animation owner or variable
height virtual list is introduced. Existing Reduce Motion, realization bounds
and the 64-key immediate-settlement limit apply.
Cache each ghost anchor's final index when membership changes. If scrolling
virtualizes that anchor away, host its exit paint at the first realized slot,
translating from the anchor's animated position. This preserves still-visible
exits without scanning the full list each frame.
Rehosted leading ghosts paint before that slot's live content; otherwise scrolling
would reverse their overlap order. A frozen-clock native pixel comparison checks
the surviving row before and after the anchor leaves the viewport.

Native directory scenarios cover nested and compressed paths, independent equal
basenames, pointer/keyboard navigation, diff preservation, Path/Tree round trips,
Generated independence, both workspace widths, interpolated geometry, inert
exit paint (including an offscreen anchor), moving live targets, same-commit
unavailable/read-restored continuity and Reduce Motion.

