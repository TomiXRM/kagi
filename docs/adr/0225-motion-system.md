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
behavior gate. A locked screen prevents Tier B visual comparison; math tests
and hidden native windows must not be described as that evidence.

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
stable path identity; Generated fold children use fixed-height splice motion.
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

