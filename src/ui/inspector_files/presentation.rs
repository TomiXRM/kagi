//! Keyed presentation and bounded realization for the Inspector's fixed rows.
use super::super::{diffstat_bar, e2e};
use super::{Entry, FileRow, Presentation};
use gpui::{div, prelude::*, px, relative, rgb, Pixels, SharedString};
use kagi_ui_core::motion::{self, Easing, KeyedTransition, RowSplice};
use kagi_ui_core::{
    file_tree::status_badge,
    theme::{self, theme},
};
use std::{collections::HashMap, time::Instant};
#[derive(Default)]
pub(super) struct Motion {
    pub(super) splice: RowSplice<SharedString>,
    // Only selected and still-fading paths are retained.
    pub(super) washes: HashMap<SharedString, KeyedTransition<bool>>,
    pub(super) selected: Option<SharedString>,
    pub(super) initialized: bool,
    pub(super) order: HashMap<SharedString, usize>,
    pub(super) ghosts: Vec<Ghost>,
}
pub(super) struct Ghost {
    pub(super) entry: Entry,
    pub(super) key: SharedString,
    pub(super) after: Option<SharedString>,
    pub(super) after_index: usize,
}
impl Motion {
    pub(super) fn sync_entries(
        &mut self,
        old: &[Entry],
        new: &[Entry],
        order: &HashMap<SharedString, usize>,
    ) {
        let now = motion::now();
        let old_keys: std::collections::HashSet<_> = old.iter().map(Entry::key).collect();
        let new_keys: std::collections::HashSet<_> = new.iter().map(Entry::key).collect();
        if self.splice.splice(
            new.iter()
                .map(Entry::key)
                .filter(|key| !old_keys.contains(key)),
            old.iter()
                .map(Entry::key)
                .filter(|key| !new_keys.contains(key)),
            now,
            motion::instant(),
        ) {
            self.ghosts.clear();
            return;
        }
        self.ghosts.retain(|ghost| {
            self.splice
                .ghost(&ghost.key, None, std::iter::empty(), px(1.), now)
                .is_some()
        });
        self.ghosts.extend(
            old.iter()
                .filter(|entry| !new_keys.contains(&entry.key()))
                .cloned()
                .map(|entry| Ghost {
                    key: entry.key(),
                    entry,
                    after: None,
                    after_index: 0,
                }),
        );
        self.ghosts.sort_by_key(|ghost| order[&ghost.key]);
        let mut surviving = new.iter().enumerate().peekable();
        let mut after = None;
        let mut after_index = 0;
        for ghost in &mut self.ghosts {
            let rank = order[&ghost.key];
            while surviving
                .peek()
                .is_some_and(|(_, entry)| order[&entry.key()] < rank)
            {
                after = surviving.next().map(|(index, entry)| {
                    after_index = index;
                    entry.key()
                });
            }
            ghost.after = after.clone();
            ghost.after_index = after_index;
        }
    }
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
        let key = entry.key();
        self.order.get(&key).map_or(
            motion::RowSample {
                offset: px(0.),
                opacity: 1.,
            },
            |rank| {
                self.splice
                    .row(&key, pitch, now, |changed| self.order[changed] < *rank)
            },
        )
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
// Shared painted content; ghosts deliberately do not receive RowList, AX roles,
// focus handles, pointer handlers, tooltips, or semantic-list recorder calls.
pub(super) fn file_content(
    row: &FileRow,
    presentation: Presentation,
    wash: f32,
    probes: bool,
) -> gpui::AnyElement {
    let index = row.index;
    let file = &row.source.files[index];
    let (badge, color, _) = status_badge(Some(&file.change), false);
    // Intent/AX changes immediately. In-flight labels share readable opaque
    // ink; settled primary/secondary styles remain exactly as before.
    let mut fill = theme().selected_for_window(presentation.active);
    fill.a *= wash;
    let background = gpui::Hsla::from(rgb(theme().panel)).blend(fill);
    let ink = diffstat_bar::file_selection_ink(wash, presentation.active, background);
    let primary = ink.map(|(primary, _)| primary);
    let foreground = ink.map(|(_, secondary)| secondary);
    let label = gpui::Hsla::from(rgb(theme().text_main));
    let badge_color = gpui::Hsla::from(rgb(color));
    let stat = if probes {
        diffstat_bar::diffstat_unit(index, file.stat.as_ref(), foreground)
    } else {
        diffstat_bar::diffstat_unit_inert(file.stat.as_ref(), foreground)
    };
    #[cfg(feature = "gui-e2e")]
    let stat = if probes {
        e2e::measure_leaf(format!("inspector-file-stat-{index}"), stat)
    } else {
        stat.into_any_element()
    };
    div()
        .relative()
        .flex()
        .w_full()
        .items_center()
        .gap_1()
        .pl(theme::scaled_px(row.depth as f32 * 12.))
        .h_full()
        .line_height(relative(1.2))
        .rounded_sm()
        .bg(fill)
        .child(
            div()
                .w(theme::scaled_px(16.))
                .flex_shrink_0()
                .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
                .text_color(primary.unwrap_or(badge_color))
                .child(SharedString::from(badge))
                .when(cfg!(feature = "gui-e2e") && probes, |badge| {
                    badge
                        .relative()
                        .child(e2e::measure_inside(format!("inspector-file-badge-{index}")))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
                .text_color(primary.unwrap_or(label))
                .truncate()
                .child(row.name.clone())
                .when(cfg!(feature = "gui-e2e") && probes, |name| {
                    name.relative()
                        .child(e2e::measure_inside(format!("inspector-file-name-{index}")))
                }),
        )
        .child(stat)
        .when(cfg!(feature = "gui-e2e"), |content| {
            content.child(e2e::measure_inside(format!("inspector-file-paint-{index}")))
        })
        .when(cfg!(feature = "gui-e2e") && !probes, |content| {
            content.child(e2e::measure_inside(format!("inspector-file-ghost-{index}")))
        })
        .into_any_element()
}

pub(super) fn directory_content(name: &SharedString, depth: usize, expanded: bool) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .h_full()
        .pl(theme::scaled_px(depth as f32 * 12.))
        .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
        .text_color(rgb(theme().change_dir))
        .child(
            div()
                .w(theme::scaled_px(16.))
                .flex_shrink_0()
                .child(if expanded { "▾" } else { "▸" }),
        )
        .child(div().min_w(px(0.)).truncate().child(name.clone()))
}
