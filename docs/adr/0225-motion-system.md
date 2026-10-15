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
  `default()` per list owner. On a logical change call
  `retarget(keys, present, now, instant)` with inserted keys (`present=true`) or
  removed keys (`false`); do not call this on viewport mounts or each render.
  Existing keys reverse from their current value; new batches do not restart
  unaffected keys. Duplicate keys cannot double the displacement.
  Call `advance(now, instant)` each render to retire settled keys or clear all
  motion under instant policy; its bool is the owning view's frame demand.
  `row(key, row_height, now, precedes)` returns `RowSample { offset, opacity }`.
  The callback identifies active keys before that row in the caller's logical
  order, including retained removals. Cache ordering when the list changes;
  do not scan the full list per callback. Only active keys are stored.
- For removal, retain the real removed content and its logical order in the
  view until `ghost(key, after, between, row_height, now)` returns `None`.
  `after` is the nearest surviving predecessor key (or `None` for list start);
  `between` supplies removed keys between that anchor and this ghost.
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
