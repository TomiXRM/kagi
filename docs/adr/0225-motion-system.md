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
5. **Graph insertion is approach A.** Keep fixed-height `uniform_list`; publish
   the final list length immediately. Each active inserted key contributes
   `-row_height * (1 - eased_progress)` to rows below it; sum contributions for
   multiple insertions. The inserted row fades from zero to full opacity.
   Offsets settle at zero. This is presentation displacement, not animated row
   height or a variable-list migration. The caller derives below/above from the
   current logical order and supplies the zoomed row height.
6. **Identity is logical, never positional.** Retain row presentation by
   `CommitId`, worktree path or file path, scoped to the owning session/surface.
   List indices may locate geometry but are never animation identity. Remove
   settled insertion state and state for removed/replaced owners. Selection
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
- `InsertOffset<K>`: construct once when a logical key actually arrives, not on
  every render or viewport mount. For each affected row, `sample(row_key, below,
  row_height, now, instant)` returns offset, opacity and done. Track only active
  insertions; derive ordering once when the published list changes rather than
  rescanning the entire list for every painted row. Discard done entries.
- `Reveal`: `sync(open, now, instant)` before `sample(natural_height, now)`;
  preserve existing open-state ownership. `visible` and `animating` also support
  the existing pane clipping integration.
- `Axis`, `Anchor`, `clip`: unchanged pane clipping implementation, now shared.

These are view presentation primitives, not another state store, asynchronous
animation service or domain model. Samples do not allocate; key ownership and
active collection lifetimes belong to the caller.

## Validation boundary

Ordinary math tests cover key generations, interruption, exact pane endpoints,
multiple insertion offsets, measured heights and closing retention. Existing
native motion scenarios migrate their frozen-clock calls without changing their
behavior coverage. A separate oracle repair allows the existing 1px tolerance
for five settled-width checks: unchanged dev reproduced the exact comparison
of pixel-snapped 327.5px against unsnapped 327.6px. Native Tier A remains the
behavior gate. A locked screen prevents Tier B visual comparison; math tests
and hidden native windows must not be described as that evidence.
