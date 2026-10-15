//! Graph-only presentation, owned by the repository session's TabUiState.
use super::*;
use gpui::Pixels;
use kagi_ui_core::motion::{self, Easing, KeyedTransition, RowSample, RowSplice};
use std::{collections::HashMap, path::PathBuf, time::Instant};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RowKey {
    Commit(CommitId),
    Wip(PathBuf),
    Stash(CommitId),
}

#[derive(Clone)]
pub(super) struct WipPresentation {
    pub key: RowKey,
    pub params: WipRowParams,
    pub lane: Option<crate::graph::WipAnchor>,
    pub passing: Vec<(usize, usize)>,
}

struct Ghost {
    key: RowKey,
    row: Option<WipPresentation>,
    after: Option<RowKey>,
    between: Vec<RowKey>,
    at: usize,
}

#[derive(Default)]
pub(crate) struct GraphMotion {
    selection: HashMap<RowKey, KeyedTransition<bool>>,
    selected: Option<RowKey>,
    initialized: bool,
    signature: Option<(usize, usize, u64, bool)>,
    pub full_read: bool,
    pub reset: bool,
    order: Vec<RowKey>,
    logical: Vec<RowKey>,
    positions: HashMap<RowKey, usize>,
    final_positions: HashMap<RowKey, usize>,
    wips: Vec<WipPresentation>,
    ghosts: Vec<Ghost>,
    splice: RowSplice<RowKey>,
}

impl GraphMotion {
    pub(super) fn sync_selection(
        &mut self,
        selected: Option<RowKey>,
        now: Instant,
        instant: bool,
    ) -> bool {
        let instant = instant || !self.initialized || self.reset;
        self.initialized = true;
        if let Some(key) = &selected {
            self.selection
                .entry(key.clone())
                .or_insert_with(|| KeyedTransition::new(false, 0.));
        }
        self.selection.retain(|key, transition| {
            let on = selected.as_ref() == Some(key);
            transition.retarget(
                on,
                if on { 1. } else { 0. },
                now,
                motion::TRANSITION,
                Easing::OutCubic,
                instant,
            );
            on || !transition.sample(now).done
        });
        self.selected = selected;
        self.selection.values().any(|t| !t.sample(now).done)
    }

    pub(crate) fn selection(&self, key: &RowKey) -> f32 {
        self.selection
            .get(key)
            .map_or(0., |t| t.sample(motion::now()).value)
    }

    pub(crate) fn selected(&self, key: &RowKey) -> bool {
        self.selected.as_ref() == Some(key)
    }

    /// Identity/order caches are rebuilt only on a new read or a small prefix change,
    /// never on an animation frame. Indices locate geometry, not transition identity.
    pub(super) fn sync_rows(
        &mut self,
        view: &TabViewState,
        generation: u64,
        wips: Vec<WipPresentation>,
        scroll: &UniformListScrollHandle,
        height: Pixels,
        now: Instant,
        instant: bool,
    ) -> bool {
        // Before publishing a changed length, the handle still describes the
        // previous list. Use its snapped pitch when preserving its reading anchor.
        let height = scroll
            .0
            .borrow()
            .last_item_size
            .filter(|_| !self.order.is_empty())
            .map_or(height, |size| {
                size.contents.height / self.order.len() as f32
            });
        let signature = (
            view.rows.as_ptr() as usize,
            view.rows.len(),
            generation,
            view.branch_solo.is_some(),
        );
        let changed = self.reset
            || self.signature != Some(signature)
            || self.wips.len() != wips.len()
            || self.wips.iter().zip(&wips).any(|(a, b)| a.key != b.key);
        if changed {
            let mut order: Vec<_> = wips.iter().map(|w| w.key.clone()).collect();
            order.extend(view.stash_graph_rows.iter().filter_map(|r| {
                view.stashes
                    .iter()
                    .find(|s| s.index == r.index)
                    .map(|s| RowKey::Stash(s.target.clone()))
            }));
            order.extend(view.rows.iter().map(|r| RowKey::Commit(r.id.clone())));
            let new_positions: HashMap<_, _> = order
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, key)| (key, i))
                .collect();
            let inserted: Vec<_> = order
                .iter()
                .filter(|key| !self.final_positions.contains_key(*key))
                .cloned()
                .collect();
            let removed: Vec<_> = self
                .order
                .iter()
                .filter(|key| !new_positions.contains_key(*key))
                .cloned()
                .collect();
            let handle = scroll.0.borrow();
            let screen = (handle.base_handle.bounds().size.height / height)
                .floor()
                .max(1.) as usize;
            let offset = handle.base_handle.offset();
            let top = (-offset.y / height).floor().max(0.) as usize;
            let mut anchored = false;
            // Pixels uses total_cmp: scroll_to_item(0) produces -0, which must
            // remain the unanchored top rather than being ordered below +0.
            if f32::from(offset.y) < 0. && handle.deferred_scroll_to_item.is_none() {
                if let Some(new_top) = self.order.get(top).and_then(|key| new_positions.get(key)) {
                    if *new_top != top {
                        let mut next = offset;
                        next.y -= height * (*new_top as f32 - top as f32);
                        handle.base_handle.set_offset(next);
                        anchored = true;
                    }
                }
            }
            let only_wip = inserted
                .iter()
                .chain(&removed)
                .all(|key| matches!(key, RowKey::Wip(_)));
            let animate = self.signature.is_some()
                && !self.reset
                && !instant
                && !anchored
                && view.branch_solo.is_none()
                && (only_wip || self.full_read)
                && inserted.len() + removed.len() <= screen
                && self
                    .order
                    .iter()
                    .filter(|key| new_positions.contains_key(*key))
                    .eq(order
                        .iter()
                        .filter(|key| self.final_positions.contains_key(*key)));
            if !animate {
                self.splice = RowSplice::default();
                self.ghosts.clear();
            } else {
                for key in &removed {
                    let row = self.wips.iter().find(|w| &w.key == key).cloned();
                    self.ghosts.push(Ghost {
                        key: key.clone(),
                        row,
                        after: None,
                        between: Vec::new(),
                        at: 0,
                    });
                }
                self.ghosts.retain(|g| !new_positions.contains_key(&g.key));
                if self.splice.splice(inserted, removed, now, false) {
                    self.ghosts.clear();
                }
            }
            // Only the bounded ghost set needs predecessor bookkeeping. The
            // whole commit order is traversed linearly once per read, not frame.
            let mut preceding = None;
            let mut between = Vec::new();
            for key in &self.logical {
                if new_positions.contains_key(key) {
                    preceding = Some(key.clone());
                    between.clear();
                } else if let Some(ghost) = self.ghosts.iter_mut().find(|g| &g.key == key) {
                    ghost.after = preceding.clone();
                    ghost.between.clone_from(&between);
                    ghost.at = preceding
                        .as_ref()
                        .and_then(|k| new_positions.get(k))
                        .copied()
                        .unwrap_or(0);
                    between.push(key.clone());
                }
            }
            // Existing ghosts and newly removed predecessors can share an anchor.
            // Preserve their logical order, not the order of removal batches.
            self.ghosts
                .sort_by_key(|ghost| self.positions.get(&ghost.key).copied());
            let mut grouped: HashMap<Option<RowKey>, Vec<RowKey>> = HashMap::new();
            for ghost in &self.ghosts {
                grouped
                    .entry(ghost.after.clone())
                    .or_default()
                    .push(ghost.key.clone());
            }
            let mut logical = grouped.remove(&None).unwrap_or_default();
            for key in &order {
                logical.push(key.clone());
                if let Some(ghosts) = grouped.remove(&Some(key.clone())) {
                    logical.extend(ghosts);
                }
            }
            self.positions = logical
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, key)| (key, i))
                .collect();
            self.logical = logical;
            self.order = order;
            self.signature = Some(signature);
            self.final_positions = new_positions;
        }
        self.wips = wips;
        self.full_read = false;
        self.reset = false;
        let moving = self.splice.advance(now, instant);
        self.ghosts.retain(|g| {
            self.splice
                .ghost(&g.key, g.after.as_ref(), &g.between, height, now)
                .is_some()
        });
        moving
    }

    pub(crate) fn key(&self, index: usize) -> Option<&RowKey> {
        self.order.get(index)
    }

    pub(super) fn sample(&self, key: &RowKey, height: Pixels) -> RowSample {
        let position = self.positions.get(key);
        self.splice.row(key, height, motion::now(), |other| {
            self.positions.get(other) < position
        })
    }

    /// Include only final slots whose animated content intersects the requested
    /// viewport. GPUI's uniform_list has no overscan; out-of-flow edge slots
    /// remain necessary until their displacement settles. The whole expansion
    /// is bounded by the shared active-splice cap, not repository history.
    pub(super) fn presentation_range(
        &self,
        requested: std::ops::Range<usize>,
        height: Pixels,
    ) -> std::ops::Range<usize> {
        let mut visible = requested.clone();
        let mut remaining = motion::MAX_ANIMATED_ROWS;
        if requested.is_empty() {
            return visible;
        }
        // A ghost can remain visible after its predecessor itself scrolls out.
        // Include that mounting slot once, without making the ghost interactive.
        for ghost in self.ghosts.iter().filter(|ghost| ghost.row.is_some()) {
            let Some(sample) = self.splice.ghost(
                &ghost.key,
                ghost.after.as_ref(),
                &ghost.between,
                height,
                motion::now(),
            ) else {
                continue;
            };
            let anchor = ghost
                .after
                .as_ref()
                .map_or(px(0.), |key| self.sample(key, height).offset);
            let top = height * ghost.at as f32 + anchor + sample.offset;
            if top + height > height * requested.start as f32 && top < height * requested.end as f32
            {
                let start = visible.start.min(ghost.at);
                let end = visible.end.max(ghost.at + 1).min(self.order.len());
                let extra = visible.start - start + end - visible.end;
                if extra <= remaining {
                    visible = start..end;
                    remaining -= extra;
                }
            }
        }
        while visible.start > 0 && remaining > 0 {
            let index = visible.start - 1;
            let Some(key) = self.order.get(index) else {
                break;
            };
            if height * (index + 1) as f32 + self.sample(key, height).offset
                <= height * requested.start as f32
            {
                break;
            }
            visible.start -= 1;
            remaining -= 1;
        }
        while visible.end < self.order.len() && remaining > 0 {
            let index = visible.end;
            if height * index as f32 + self.sample(&self.order[index], height).offset
                >= height * requested.end as f32
            {
                break;
            }
            visible.end += 1;
            remaining -= 1;
        }
        visible
    }

    pub(super) fn ghosts_after(
        &self,
        index: usize,
        height: Pixels,
    ) -> impl Iterator<Item = (&WipPresentation, Pixels, f32)> {
        self.ghosts.iter().filter_map(move |g| {
            if g.at != index {
                return None;
            }
            let row = g.row.as_ref()?;
            let sample =
                self.splice
                    .ghost(&g.key, g.after.as_ref(), &g.between, height, motion::now())?;
            let anchor_offset = g
                .after
                .as_ref()
                .map_or(px(0.), |key| self.sample(key, height).offset);
            Some((row, anchor_offset + sample.offset, sample.opacity))
        })
    }
}
