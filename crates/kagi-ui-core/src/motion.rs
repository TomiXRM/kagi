//! View-owned motion, extracted from Kagi's pane transitions (ADR-0225).
//!
//! Read `now()` and `instant()` once per render, sync retained state, then call
//! `request_frame` from the owning view if anything is moving. Do not depend on
//! a virtualized/scroll-contained element being laid out to drive the clock.

use std::time::{Duration, Instant};

use gpui::{div, prelude::*, Div, ElementId, Pixels, Stateful, Window};

pub const OPEN: Duration = Duration::from_millis(180);
pub const CLOSE: Duration = Duration::from_millis(150);
pub const TRANSITION: Duration = Duration::from_millis(180);

#[cfg(feature = "gui-e2e")]
thread_local! {
    static CLOCK: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// Freeze all shared motion on this UI thread. `None` restores runner-instant
/// policy. Scenarios must restore `None` before leaving.
#[cfg(feature = "gui-e2e")]
pub fn set_clock_for_e2e(now: Option<Instant>) {
    CLOCK.with(|clock| clock.set(now));
}

pub fn now() -> Instant {
    #[cfg(feature = "gui-e2e")]
    if let Some(now) = CLOCK.with(|clock| clock.get()) {
        return now;
    }
    Instant::now()
}

/// Effective system OR app Reduce Motion, plus default-instant GUI tests.
pub fn instant() -> bool {
    #[cfg(feature = "gui-e2e")]
    if CLOCK.with(|clock| clock.get()).is_none() {
        return true;
    }
    crate::theme::reduce_motion()
}

/// Call in the owning view's render, not inside the scrolling row's layout.
/// Frozen clocks are advanced by the scenario, so they need no automatic ticks.
pub fn request_frame(window: &Window, animating: bool) {
    if !animating || instant() {
        return;
    }
    #[cfg(feature = "gui-e2e")]
    if CLOCK.with(|clock| clock.get()).is_some() {
        return;
    }
    window.request_animation_frame();
}

#[derive(Clone, Copy, Debug)]
pub enum Easing {
    InCubic,
    OutCubic,
}

impl Easing {
    pub fn apply(self, progress: f32) -> f32 {
        let progress = progress.clamp(0., 1.);
        match self {
            Self::InCubic => progress.powi(3),
            Self::OutCubic => 1. - (1. - progress).powi(3),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TransitionSample {
    /// Interpolated value, always in 0..=1.
    pub value: f32,
    /// Elapsed fraction of this generation (before easing), in 0..=1.
    pub progress: f32,
    pub done: bool,
}

/// One retained scalar transition. Keys identify target changes, not row indices.
/// A row owner can retain one under its CommitId/path and use a selected bool as
/// the target key. Retargeting samples the current value, so reversal never jumps.
#[derive(Clone, Debug)]
pub struct KeyedTransition<K> {
    key: K,
    generation: u64,
    from: f32,
    target: f32,
    started: Option<Instant>,
    duration: Duration,
    easing: Easing,
}

impl<K: PartialEq> KeyedTransition<K> {
    pub fn new(key: K, value: f32) -> Self {
        Self {
            key,
            generation: 0,
            from: value.clamp(0., 1.),
            target: value.clamp(0., 1.),
            started: None,
            duration: Duration::ZERO,
            easing: Easing::OutCubic,
        }
    }

    pub fn key(&self) -> &K {
        &self.key
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns whether the target changed. Repeating a key/target never restarts
    /// it; `instant` still settles an already-running generation immediately.
    pub fn retarget(
        &mut self,
        key: K,
        target: f32,
        now: Instant,
        duration: Duration,
        easing: Easing,
        instant: bool,
    ) -> bool {
        let target = target.clamp(0., 1.);
        if self.key == key && self.target == target {
            if instant {
                self.started = None;
            }
            return false;
        }
        self.from = self.sample(now).value;
        self.key = key;
        self.target = target;
        self.generation = self.generation.wrapping_add(1);
        self.duration = duration;
        self.easing = easing;
        self.started = (!instant && !duration.is_zero()).then_some(now);
        true
    }

    pub fn sample(&self, now: Instant) -> TransitionSample {
        let progress = self.started.map_or(1., |started| {
            (now.saturating_duration_since(started).as_secs_f32() / self.duration.as_secs_f32())
                .clamp(0., 1.)
        });
        let done = progress >= 1.;
        TransitionSample {
            value: if done {
                self.target
            } else {
                self.from + (self.target - self.from) * self.easing.apply(progress)
            },
            progress,
            done,
        }
    }
}

/// Measured content reveal. First render is settled; later toggles preserve the
/// current fraction and scale the duration by the distance remaining.
#[derive(Clone, Debug, Default)]
pub struct Reveal {
    transition: Option<KeyedTransition<bool>>,
}

#[derive(Clone, Copy, Debug)]
pub struct RevealSample {
    pub fraction: f32,
    pub height: Pixels,
    /// Keep the child mounted until closing finishes (including the first frame).
    pub retain_content: bool,
    pub done: bool,
}

impl Reveal {
    pub fn target(&self) -> Option<bool> {
        self.transition.as_ref().map(|transition| *transition.key())
    }

    pub fn sync(&mut self, open: bool, now: Instant, instant: bool) {
        let target = if open { 1. } else { 0. };
        let Some(transition) = self.transition.as_mut() else {
            self.transition = Some(KeyedTransition::new(open, target));
            return;
        };
        let from = transition.sample(now).value;
        transition.retarget(
            open,
            target,
            now,
            reveal_duration(open, from),
            if open {
                Easing::OutCubic
            } else {
                Easing::InCubic
            },
            instant,
        );
    }

    pub fn visible(&self, now: Instant) -> f32 {
        self.transition
            .as_ref()
            .map_or(0., |transition| transition.sample(now).value)
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.transition
            .as_ref()
            .is_some_and(|transition| !transition.sample(now).done)
    }

    /// Supply the child's measured natural height, not its clipped height.
    /// Sync with `instant()` each frame before sampling, including on preference changes.
    pub fn sample(&self, full_height: Pixels, now: Instant) -> RevealSample {
        let fraction = self.visible(now);
        let done = !self.animating(now);
        RevealSample {
            fraction,
            height: full_height * fraction,
            retain_content: self.target() == Some(true) || !done,
            done,
        }
    }
}

fn reveal_duration(open: bool, from: f32) -> Duration {
    let (full, left) = if open {
        (OPEN, 1. - from)
    } else {
        (CLOSE, from)
    };
    // Whole microseconds preserve the pane's exact 180/150ms end boundaries.
    Duration::from_micros((full.as_micros() as f32 * left.clamp(0., 1.)).round() as u64)
}

/// Active changes to a fixed-height list. Publish the final list immediately;
/// retain removed content separately as ghosts until `ghost` returns `None`.
/// Only changing logical keys are stored, never indices or a copy of the list.
#[derive(Clone, Debug)]
pub struct RowSplice<K> {
    rows: std::collections::HashMap<K, KeyedTransition<bool>>,
}

impl<K> Default for RowSplice<K> {
    fn default() -> Self {
        Self {
            rows: std::collections::HashMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RowSample {
    /// Add to the row's final-list position; do not change the uniform row height.
    pub offset: Pixels,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct GhostSample<'a, K> {
    /// Previous surviving row, or `None` for the list-start boundary.
    pub after: Option<&'a K>,
    /// Relative to that row's animated top, or the list start when unanchored.
    pub offset: Pixels,
    pub opacity: f32,
}

impl<K: Eq + std::hash::Hash> RowSplice<K> {
    /// Apply a batch of inserted (`present=true`) or removed (`false`) keys.
    /// Existing keys reverse from their current presence/opacity; unaffected
    /// transitions continue, so overlapping batches do not restart one another.
    /// Call only on logical changes, not on viewport mounts or each render.
    pub fn retarget(
        &mut self,
        keys: impl IntoIterator<Item = K>,
        present: bool,
        now: Instant,
        instant: bool,
    ) {
        for key in keys {
            if instant {
                self.rows.remove(&key);
                continue;
            }
            let transition = self
                .rows
                .entry(key)
                .or_insert_with(|| KeyedTransition::new(!present, if present { 0. } else { 1. }));
            transition.retarget(
                present,
                if present { 1. } else { 0. },
                now,
                TRANSITION,
                Easing::OutCubic,
                false,
            );
        }
    }

    /// Call each render before sampling. Settled changes are retired; instant
    /// policy clears all motion permanently. Returns whether to request a frame.
    pub fn advance(&mut self, now: Instant, instant: bool) -> bool {
        if instant {
            self.rows.clear();
        } else {
            self.rows
                .retain(|_, transition| !transition.sample(now).done);
        }
        !self.rows.is_empty()
    }

    /// `precedes(key)` tells whether an active key precedes this row in the
    /// caller's logical order, including retained removals. Cache that ordering
    /// when the list changes; do not scan the whole list inside this callback.
    pub fn row(
        &self,
        key: &K,
        row_height: Pixels,
        now: Instant,
        mut precedes: impl FnMut(&K) -> bool,
    ) -> RowSample {
        let mut offset = 0.;
        let mut opacity = 1.;
        for (changed, transition) in &self.rows {
            let presence = transition.sample(now).value;
            if changed == key {
                opacity = presence;
            } else if precedes(changed) {
                // Insertions reserve their final slot now; removals no longer
                // have one. This baseline switch preserves position on reversal.
                offset += presence - if *transition.key() { 1. } else { 0. };
            }
        }
        RowSample {
            offset: row_height * offset,
            opacity,
        }
    }

    /// Position a retained removed row after its nearest surviving predecessor.
    /// `between` contains removed logical keys between that predecessor and this
    /// ghost, in the caller's retained order. Supply updated anchors after every
    /// splice, including when an earlier anchor is itself removed or reinserted.
    /// Add `offset` to the predecessor's animated top (or list start for `None`).
    /// No key or placement is copied, and a reversed/settled ghost is not drawn.
    pub fn ghost<'a>(
        &self,
        key: &K,
        after: Option<&'a K>,
        between: impl IntoIterator<Item = &'a K>,
        row_height: Pixels,
        now: Instant,
    ) -> Option<GhostSample<'a, K>> {
        let transition = self.rows.get(key)?;
        let sample = transition.sample(now);
        if *transition.key() || sample.done {
            return None;
        }
        let mut offset = after.map_or(0., |anchor| {
            self.rows
                .get(anchor)
                .map_or(1., |transition| transition.sample(now).value)
        });
        for preceding in between {
            if let Some(transition) = self.rows.get(preceding) {
                if !*transition.key() {
                    offset += transition.sample(now).value;
                }
            }
        }
        Some(GhostSample {
            after,
            offset: row_height * offset,
            opacity: sample.value,
        })
    }
}

/// Which dimension of a clipped outer box grows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

/// Edge of the outer box from which full-sized content hangs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    End,
}

/// Clip an unchanged full-size child to `full * fraction`. Used by panes and
/// measured accordion bodies; never vary row height inside `uniform_list`.
pub fn clip(
    id: impl Into<ElementId>,
    axis: Axis,
    anchor: Anchor,
    full: Pixels,
    fraction: f32,
    child: impl IntoElement,
) -> Stateful<Div> {
    let cut = full * fraction.clamp(0., 1.);
    let outer = div().id(id).relative().flex_shrink_0().overflow_hidden();
    let inner = div().absolute();
    let (outer, inner) = match axis {
        Axis::Vertical => (outer.w_full().h(cut), inner.left_0().right_0().h(full)),
        Axis::Horizontal => (outer.h_full().w(cut), inner.top_0().bottom_0().w(full)),
    };
    let inner = match (axis, anchor) {
        (Axis::Vertical, Anchor::Start) => inner.top_0(),
        (Axis::Vertical, Anchor::End) => inner.bottom_0(),
        (Axis::Horizontal, Anchor::Start) => inner.left_0(),
        (Axis::Horizontal, Anchor::End) => inner.right_0(),
    };
    outer.child(inner.child(child))
}

#[cfg(test)]
mod tests;
