//! Keyed presentation and bounded realization for the Inspector's fixed rows.
use super::{Entry, FileRow};
use gpui::{px, Pixels, SharedString};
use kagi_ui_core::motion::{self, Easing, KeyedTransition, RowSplice};
use std::{collections::HashMap, time::Instant};
#[derive(Default)]
pub(super) struct Motion {
    pub(super) splice: RowSplice<SharedString>,
    // Only selected and still-fading paths are retained.
    pub(super) washes: HashMap<SharedString, KeyedTransition<bool>>,
    pub(super) selected: Option<SharedString>,
    pub(super) initialized: bool,
    pub(super) order: HashMap<SharedString, usize>,
    pub(super) ghosts: Vec<FileRow>,
}
impl Motion {
    pub(super) fn sync_selection(
        &mut self,
        selected: Option<SharedString>,
        now: Instant,
        instant: bool,
    ) -> bool {
        let first = !self.initialized;
        self.initialized = true;
        if let Some(path) = selected.as_ref() {
            self.washes
                .entry(path.clone())
                .or_insert_with(|| KeyedTransition::new(false, 0.));
        }
        self.selected = selected;
        let selected = &self.selected;
        let mut moving = false;
        self.washes.retain(|path, wash| {
            let target = selected.as_ref() == Some(path);
            wash.retarget(
                target,
                if target { 1. } else { 0. },
                now,
                motion::TRANSITION,
                Easing::OutCubic,
                instant || first,
            );
            let sample = wash.sample(now);
            moving |= !sample.done;
            target || !sample.done
        });
        moving
    }
    pub(super) fn sample(&self, entry: &Entry, pitch: Pixels, now: Instant) -> motion::RowSample {
        match entry {
            Entry::File(row) => {
                let path = &row.source.files[row.index].path;
                let rank = self.order[path];
                self.splice
                    .row(path, pitch, now, |key| self.order[key] < rank)
            }
            Entry::More(_) => self
                .splice
                .row(&SharedString::from("\0tail"), pitch, now, |_| true),
            _ => motion::RowSample {
                offset: px(0.),
                opacity: 1.,
            },
        }
    }

    // Final slots alone omit incoming contents displaced into the viewport.
    // Match Changes: inspect only the bounded splice fringe, never the full list.
    pub(super) fn presentation_range(
        &self,
        entries: &[Entry],
        requested: std::ops::Range<usize>,
        pitch: Pixels,
        now: Instant,
    ) -> std::ops::Range<usize> {
        let mut visible = requested.clone();
        if visible.is_empty() {
            return visible;
        }
        let mut remaining = motion::MAX_ANIMATED_ROWS;
        while visible.start > 0 && remaining > 0 {
            let i = visible.start - 1;
            if pitch * (i + 1) as f32 + self.sample(&entries[i], pitch, now).offset
                <= pitch * requested.start as f32
            {
                break;
            }
            visible.start = i;
            remaining -= 1;
        }
        while visible.end < entries.len() && remaining > 0 {
            let i = visible.end;
            if pitch * i as f32 + self.sample(&entries[i], pitch, now).offset
                >= pitch * requested.end as f32
            {
                break;
            }
            visible.end += 1;
            remaining -= 1;
        }
        visible
    }
}
