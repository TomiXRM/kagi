//! Graph sidebar row model and item renderers (#864).

use kagi_ui_core::motion::{self, Easing, KeyedTransition, Reveal, RowSplice};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{div, prelude::*, px, rgb, Context, Entity, SharedString, UniformListScrollHandle};
use gpui_component::input::InputState;
use gpui_component::tooltip::Tooltip;

use kagi_git::CommitId;

use super::sidebar_ref_rows::{
    build_group_row, build_section_header, build_stash_row, build_tag_row,
};
#[cfg(test)]
use super::sidebar_rows::{
    build_sidebar_rows, group_by_prefix, group_key, group_remotes, remote_group_key, remote_key,
    split_first_segment, GroupRow, RemoteRow,
};
use super::theme::{self, theme};
use super::{BranchDrag, BranchDragGhost, KagiApp, Msg};
use kagi_ui_core::tooltip::HoverTooltip as _;

/// Row slot height (unscaled) for every virtualized leaf outside WORKTREES.
///
/// `uniform_list` measures the first item of a list and applies that height to
/// all of them. Each pane owns its own list, so every row a pane's list can
/// draw (group headers and leaves alike) is pinned to that pane's slot height
/// and the list scrolls correctly regardless of which row happens to be first.
pub(super) const SIDEBAR_ROW_H: f32 = 20.0;

/// Pinned pane headings are separate from the virtualized body slots.
pub(super) const SIDEBAR_SECTION_HEADER_H: f32 = 28.0;

/// Row slot height for the WORKTREES pane. Its list only ever draws worktree
/// rows (name, path, port link and inspection badge), so it can use a taller
/// uniform height than the other panes.
pub(super) const SIDEBAR_WORKTREE_ROW_H: f32 = 24.0;

/// Font size (unscaled) for auxiliary row text — commit age, upstream counts,
/// PR badge, worktree path / port / inspection badge — quieter than the
/// shared 13px file/ref identity role.
pub(super) const SIDEBAR_AUX_TEXT: f32 = 11.0;

/// Default sidebar width in pixels (T023). Previously `mod.rs::SIDEBAR_DEFAULT`.
///
/// 240, not the original 200: branch names under LOCAL BRANCHES are grouped by
/// prefix and indented, so the common `feature/...` leaf used to be ellipsised
/// at the default width (user request). Well inside `SIDEBAR_MIN..SIDEBAR_MAX`,
/// and this is only the starting width — the divider drag still owns it for the
/// rest of the session (it is not written to `settings.json`).
const SIDEBAR_DEFAULT_WIDTH: f32 = 240.0;

#[cfg(feature = "gui-e2e")]
thread_local! {
    static FILTER_PLACEHOLDER: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

#[cfg(feature = "gui-e2e")]
pub(super) fn record_filter_placeholder(label: &'static str) {
    FILTER_PLACEHOLDER.with(|record| record.set(Some(label)));
}

#[cfg(feature = "gui-e2e")]
pub fn filter_placeholder_for_e2e() -> Option<&'static str> {
    FILTER_PLACEHOLDER.with(|record| record.get())
}

#[cfg(feature = "gui-e2e")]
#[derive(Clone, Copy, Debug)]
pub struct SidebarDrawnMotion {
    pub opacity: f32,
    pub y: f32,
    pub height: f32,
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static DRAWN_MOTION: RefCell<HashMap<(usize, String, &'static str), SidebarDrawnMotion>> = RefCell::new(HashMap::new());
}

#[cfg(feature = "gui-e2e")]
pub fn clear_sidebar_motion_for_e2e() {
    DRAWN_MOTION.with(|drawn| drawn.borrow_mut().clear());
}

#[cfg(feature = "gui-e2e")]
pub fn sidebar_motion_for_e2e(
    pane: usize,
    key: &str,
    kind: &'static str,
) -> Option<SidebarDrawnMotion> {
    DRAWN_MOTION.with(|drawn| drawn.borrow().get(&(pane, key.to_owned(), kind)).copied())
}

/// Record style opacity and actual painted geometry, not a pure-state sample.
pub(super) fn probe_row_motion(
    el: gpui::Div,
    row: &SidebarRow,
    pane: usize,
    kind: &'static str,
) -> gpui::Div {
    #[cfg(feature = "gui-e2e")]
    {
        let mut el = el;
        let key = match row {
            SidebarRow::SectionHeader { section, .. } => format!("section:{section}"),
            SidebarRow::LocalGroupHeader { key, .. }
            | SidebarRow::RemoteHeader { key, .. }
            | SidebarRow::RemoteSubGroup { key, .. } => format!("group:{key}"),
            SidebarRow::LocalBranchLeaf { name, .. } => format!("branch:{name}"),
            SidebarRow::RemoteLeaf { display, .. } => format!("remote:{display}"),
            SidebarRow::Tag { name, .. } => format!("tag:{name}"),
            SidebarRow::Worktree { path, .. } => format!("worktree:{}", path.display()),
            SidebarRow::Stash { target, .. } => format!("stash:{target}"),
        };
        let opacity = el.style().opacity.unwrap_or(1.);
        el.child(
            gpui::canvas(
                |_, _, _| {},
                move |bounds: gpui::Bounds<gpui::Pixels>, (), _, _| {
                    DRAWN_MOTION.with(|drawn| {
                        drawn.borrow_mut().insert(
                            (pane, key.clone(), kind),
                            SidebarDrawnMotion {
                                opacity,
                                y: f32::from(bounds.origin.y),
                                height: f32::from(bounds.size.height),
                            },
                        );
                    });
                },
            )
            .absolute()
            .size_full(),
        )
    }
    #[cfg(not(feature = "gui-e2e"))]
    {
        let _ = (row, pane, kind);
        el
    }
}

/// Consolidated Repository-Navigator (left sidebar) state.
///
/// App-global (not per-tab), preserved across repository reloads. `new()`
/// restores the five-pane weights and collapsed sections from settings once;
/// the filter `InputState` is created with the window before its first frame.
pub struct SidebarState {
    /// Current sidebar width in pixels (T023: user-resizable).
    pub width: f32,
    /// Independent virtual-list scroll positions; one per fixed Graph pane.
    pub scroll_handles: [UniformListScrollHandle; 5],
    /// Cached navigator rows. Five virtual lists index contiguous ranges of
    /// this single Vec, so unchanged frames only construct visible leaves.
    pub rows: Vec<SidebarRow>,
    /// Contiguous header+leaf ranges in `rows`, recomputed only on a row rebuild.
    pub pane_ranges: [std::ops::Range<usize>; 5],
    /// Relative heights for expanded pane bodies; collapse never mutates them.
    pub pane_weights: [u16; 5],
    /// Measured window-space bounds for drag calculations, including zoom.
    pub pane_geom: [std::rc::Rc<std::cell::Cell<(f32, f32)>>; 5],
    /// T-PERF-RENDER-002 (ADR-0116 Wave 2): fingerprint of the inputs that
    /// produced the cached `rows`. `render` hashes cheap input revisions,
    /// collapse sets and filter text; unchanged frames do not rebuild refs.
    /// A rebuild also refreshes the five pane ranges.
    pub rows_fingerprint: u64,
    /// #354: each row's 1-based `(position, size)` among its tree siblings,
    /// for the accessibility tree. Derived from every row, so it is cached
    /// for the `rows_fingerprint` it was computed at (`tree_positions_for`)
    /// and a scroll batch only reads it; `refresh_tree_positions` recomputes
    /// when `rows` has been rebuilt since.
    pub(super) tree_positions: Vec<(usize, usize)>,
    tree_positions_for: Option<u64>,
    /// Sole runtime owner of section collapse, restored and persisted as a mask.
    pub collapsed: HashSet<&'static str>,
    /// Window-owned filter input; initialized before the first GUI frame.
    pub filter: Option<Entity<InputState>>,
    pub(super) filter_placeholder_lang: Option<super::i18n::Lang>,
    /// Whether the navigator is shown (View → Toggle Sidebar). Default `true`.
    pub visible: bool,
    /// Transient pointer gesture, never repository data (ADR-0199).
    pub swipe: kagi_domain::sidebar_swipe::SidebarSwipe,
    /// Generation of the running settle animation. Bumped when a settle is
    /// superseded or abandoned so its frame loop retires instead of ticking
    /// the next gesture twice as fast.
    pub settle_gen: u64,
    /// The Graph / PRs / Issues cells, for the keyboard (#944).
    pub(crate) mode_focus: super::keyboard_nav::TabFocus,
    /// The panes' rows and headers, for the keyboard (#981).
    pub(crate) focus: super::sidebar_focus::SidebarFocus,
    /// Presentation only; logical rows, focus and accessibility never wait for it.
    pub(super) motion: RefCell<SidebarMotion>,
}

impl SidebarState {
    pub fn new() -> Self {
        let layout = super::settings::Settings::load().sidebar_pane_layout();
        let mut collapsed = HashSet::new();
        for (index, section) in super::sidebar_panes::SECTIONS.iter().enumerate() {
            if layout.collapsed_mask & (1 << index) != 0 {
                collapsed.insert(*section);
            }
        }
        Self {
            width: SIDEBAR_DEFAULT_WIDTH,
            scroll_handles: std::array::from_fn(|_| UniformListScrollHandle::new()),
            rows: Vec::new(),
            pane_ranges: std::array::from_fn(|_| 0..0),
            pane_weights: layout.weights,
            pane_geom: std::array::from_fn(|_| Default::default()),
            rows_fingerprint: u64::MAX,
            tree_positions: Vec::new(),
            tree_positions_for: None,
            collapsed,
            filter: None,
            filter_placeholder_lang: None,
            visible: true,
            swipe: Default::default(),
            settle_gen: 0,
            mode_focus: Default::default(),
            focus: Default::default(),
            motion: RefCell::new(SidebarMotion::default()),
        }
    }

    /// Bring `tree_positions` in line with `rows`. `rows` and
    /// `rows_fingerprint` are only replaced together (the rebuild in
    /// `render`), so a fingerprint or length mismatch means the rows were
    /// rebuilt — after a refresh, collapse / expand, filter edit or session
    /// switch — and the positions are recomputed once, O(rows) without names.
    pub(super) fn refresh_tree_positions(&mut self) {
        if self.tree_positions_for == Some(self.rows_fingerprint)
            && self.tree_positions.len() == self.rows.len()
        {
            return;
        }
        let levels: Vec<usize> = self.rows.iter().map(super::sidebar_a11y::level).collect();
        self.tree_positions = kagi_ui_core::tree_a11y::sibling_positions(&levels);
        self.tree_positions_for = Some(self.rows_fingerprint);
    }
}

impl Default for SidebarState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum SidebarMotionKey {
    Group(String),
    Branch(String),
    Remote(String),
    Tag(String),
    Worktree(std::path::PathBuf),
    Stash(CommitId, usize),
}

pub(super) type MotionKey = Rc<SidebarMotionKey>;

impl SidebarRow {
    fn motion_key(&self, stashes: &mut HashMap<CommitId, usize>) -> Option<MotionKey> {
        Some(Rc::new(match self {
            Self::SectionHeader { .. } => return None,
            Self::LocalGroupHeader { key, .. }
            | Self::RemoteHeader { key, .. }
            | Self::RemoteSubGroup { key, .. } => SidebarMotionKey::Group(key.clone()),
            Self::LocalBranchLeaf { name, .. } => SidebarMotionKey::Branch(name.clone()),
            Self::RemoteLeaf { display, .. } => SidebarMotionKey::Remote(display.clone()),
            Self::Tag { name, .. } => SidebarMotionKey::Tag(name.clone()),
            Self::Worktree { path, .. } => SidebarMotionKey::Worktree(path.clone()),
            Self::Stash { target, .. } => {
                let rank = stashes.entry(target.clone()).or_default();
                *rank += 1;
                SidebarMotionKey::Stash(target.clone(), *rank)
            }
        }))
    }
}

pub(super) struct SidebarGhost {
    pub row: SidebarRow,
    key: MotionKey,
    after: Option<MotionKey>,
    between: Vec<MotionKey>,
    pub mount: usize,
}

#[derive(Default)]
pub(super) struct SidebarPaneMotion {
    keys: Vec<MotionKey>,
    order: HashMap<MotionKey, usize>,
    ordered: Vec<MotionKey>,
    pub ghosts: Vec<SidebarGhost>,
    splice: RowSplice<MotionKey>,
    washes: HashMap<MotionKey, KeyedTransition<bool>>,
    selected: Option<MotionKey>,
    pub reveal: Reveal,
    pub natural_body: f32,
    pub closing_rows: Vec<SidebarRow>,
}

#[derive(Default)]
pub(super) struct SidebarMotion {
    owner: Option<crate::app::SessionId>,
    filter: String,
    initialized: bool,
    pub panes: [SidebarPaneMotion; 5],
}

impl SidebarPaneMotion {
    pub fn row_sample(&self, position: usize, row_h: gpui::Pixels) -> motion::RowSample {
        let Some(key) = self.keys.get(position) else {
            return motion::RowSample {
                offset: px(0.),
                opacity: 1.,
            };
        };
        let at = self.order.get(key);
        self.splice.row(key, row_h, motion::now(), |changed| {
            self.order.get(changed) < at
        })
    }

    pub fn wash(&self, position: usize) -> f32 {
        self.keys
            .get(position)
            .and_then(|key| self.washes.get(key))
            .map_or(0., |wash| wash.sample(motion::now()).value)
    }

    pub fn ghost_sample<'a>(
        &'a self,
        ghost: &'a SidebarGhost,
        row_h: gpui::Pixels,
    ) -> Option<motion::GhostSample<'a, MotionKey>> {
        self.splice.ghost(
            &ghost.key,
            ghost.after.as_ref(),
            ghost.between.iter(),
            row_h,
            motion::now(),
        )
    }

    /// Derived from this frame's existing RowList, after its focus sync. No
    /// logical selection is introduced, and reordered rows never borrow an index.
    pub fn sync_wash(&mut self, at: Option<usize>) -> bool {
        let selected = at.and_then(|at| self.keys.get(at)).cloned();
        if let Some(key) = &selected {
            self.washes
                .entry(key.clone())
                .or_insert_with(|| KeyedTransition::new(false, 0.));
        }
        self.selected = selected;
        let now = motion::now();
        let mut moving = false;
        for (key, wash) in &mut self.washes {
            let selected = self.selected.as_ref() == Some(key);
            wash.retarget(
                selected,
                if selected { 1. } else { 0. },
                now,
                motion::TRANSITION,
                Easing::OutCubic,
                motion::instant(),
            );
            moving |= !wash.sample(now).done;
        }
        moving
    }

    fn set_rows(
        &mut self,
        old: &[SidebarRow],
        new: &[SidebarRow],
        instant: bool,
        pane_toggle: bool,
    ) {
        let now = motion::now();
        let mut stashes = HashMap::new();
        let keys: Vec<_> = new
            .iter()
            .filter_map(|row| row.motion_key(&mut stashes))
            .collect();
        let present: HashSet<_> = keys.iter().cloned().collect();
        let previous: HashSet<_> = self.keys.iter().cloned().collect();
        let changed = keys.iter().filter(|key| !previous.contains(*key)).count()
            + self
                .keys
                .iter()
                .filter(|key| !present.contains(*key))
                .count();
        let instant = instant || pane_toggle || changed > 24;
        if instant {
            self.splice.advance(now, true);
            self.ghosts.clear();
        } else {
            self.ghosts.retain(|ghost| !present.contains(&ghost.key));
            for (key, row) in self.keys.iter().zip(old) {
                if !present.contains(key) && !self.ghosts.iter().any(|ghost| &ghost.key == key) {
                    self.ghosts.push(SidebarGhost {
                        key: key.clone(),
                        row: row.clone(),
                        after: None,
                        between: Vec::new(),
                        mount: 0,
                    });
                }
            }
            if self.splice.splice(
                keys.iter().filter(|key| !previous.contains(*key)).cloned(),
                self.keys
                    .iter()
                    .filter(|key| !present.contains(*key))
                    .cloned(),
                now,
                false,
            ) {
                self.ghosts.clear();
            }
        }
        // Keep removed identities in their old logical order, anchored to the
        // nearest survivor. Only a bounded active splice is merged on a rebuild.
        let mut ordered = keys.clone();
        let mut previous_key: Option<MotionKey> = None;
        for key in &self.ordered {
            if present.contains(key) {
                previous_key = Some(key.clone());
            } else if self.ghosts.iter().any(|ghost| &ghost.key == key) {
                let at = previous_key
                    .as_ref()
                    .and_then(|key| ordered.iter().position(|other| other == key))
                    .map_or(0, |at| at + 1);
                ordered.insert(at, key.clone());
                previous_key = Some(key.clone());
            }
        }
        let positions: HashMap<_, _> = keys
            .iter()
            .enumerate()
            .map(|(at, key)| (key.clone(), at))
            .collect();
        let mut anchor = None;
        let mut between = Vec::new();
        for key in &ordered {
            if present.contains(key) {
                anchor = Some(key.clone());
                between.clear();
            } else if let Some(ghost) = self.ghosts.iter_mut().find(|ghost| &ghost.key == key) {
                ghost.after = anchor.clone();
                ghost.between.clone_from(&between);
                ghost.mount = anchor
                    .as_ref()
                    .and_then(|key| positions.get(key))
                    .copied()
                    .unwrap_or(0);
                between.push(key.clone());
            }
        }
        self.order = ordered
            .iter()
            .enumerate()
            .map(|(at, key)| (key.clone(), at))
            .collect();
        self.ordered = ordered;
        self.keys = keys;
        self.washes.retain(|key, _| present.contains(key));
        if self
            .selected
            .as_ref()
            .is_some_and(|key| !present.contains(key))
        {
            self.selected = None;
        }
    }
}

impl SidebarState {
    /// Called once by the existing row-cache publisher, before replacing rows.
    pub(super) fn prepare_rows_motion(
        &self,
        rows: &[SidebarRow],
        owner: Option<crate::app::SessionId>,
        filter: &str,
        ready: bool,
    ) {
        let ranges = super::sidebar_panes::pane_ranges(rows);
        let mut state = self.motion.borrow_mut();
        let reset = !state.initialized || state.owner != owner || state.filter != filter;
        if reset {
            *state = SidebarMotion {
                owner,
                filter: filter.to_owned(),
                initialized: ready,
                ..Default::default()
            };
        }
        for (pane, range) in ranges.iter().enumerate() {
            let old_range = &self.pane_ranges[pane];
            let old = self
                .rows
                .get(old_range.start + 1..old_range.end)
                .unwrap_or_default();
            let new = rows.get(range.start + 1..range.end).unwrap_or_default();
            let old_closed = matches!(
                self.rows.get(old_range.start),
                Some(SidebarRow::SectionHeader {
                    collapsed: true,
                    ..
                })
            );
            let closed = matches!(
                rows.get(range.start),
                Some(SidebarRow::SectionHeader {
                    collapsed: true,
                    ..
                })
            );
            let pane_motion = &mut state.panes[pane];
            if !reset && closed && !old_closed {
                pane_motion.closing_rows = old.to_vec();
            } else if !closed || reset {
                pane_motion.closing_rows.clear();
            }
            pane_motion.set_rows(old, new, reset || motion::instant(), old_closed != closed);
        }
    }

    /// Owner-frame sampling, including empty/closing lists and offscreen fades.
    pub(super) fn advance_motion(&self, window: &gpui::Window, _cx: &gpui::App) {
        let now = motion::now();
        let instant = motion::instant();
        let header_h = f32::from(theme::scaled_px(SIDEBAR_SECTION_HEADER_H));
        let available: f32 = self
            .pane_geom
            .iter()
            .map(|geom| {
                let (a, b) = geom.get();
                b - a
            })
            .sum();
        let expanded_weight: f32 = super::sidebar_panes::SECTIONS
            .iter()
            .enumerate()
            .filter(|(_, key)| !self.collapsed.contains(**key))
            .map(|(pane, _)| f32::from(self.pane_weights[pane]))
            .sum();
        let closed_count = self.collapsed.len().min(5) as f32;
        let mut moving = false;
        let mut state = self.motion.borrow_mut();
        for (index, pane) in state.panes.iter_mut().enumerate() {
            let open = !self
                .collapsed
                .contains(super::sidebar_panes::SECTIONS[index]);
            if pane.reveal.target() == Some(true) && !pane.reveal.animating(now) {
                let (top, bottom) = self.pane_geom[index].get();
                pane.natural_body = (bottom - top - header_h).max(0.);
            } else if open && pane.reveal.target() == Some(false) && !pane.reveal.animating(now) {
                pane.natural_body = ((available - closed_count * header_h)
                    * f32::from(self.pane_weights[index])
                    / expanded_weight.max(1.)
                    - header_h)
                    .max(0.);
            }
            pane.reveal.sync(open, now, instant);
            moving |= pane.reveal.animating(now);
            if !pane
                .reveal
                .sample(px(pane.natural_body), now)
                .retain_content
            {
                pane.closing_rows.clear();
            }
            moving |= pane.splice.advance(now, instant);
            pane.ghosts.retain(|ghost| {
                pane.splice
                    .ghost(&ghost.key, None, std::iter::empty(), px(1.), now)
                    .is_some()
            });
            if !open {
                pane.sync_wash(None);
            }
            pane.washes.retain(|key, wash| {
                let selected = pane.selected.as_ref() == Some(key);
                wash.retarget(
                    selected,
                    if selected { 1. } else { 0. },
                    now,
                    motion::TRANSITION,
                    Easing::OutCubic,
                    instant,
                );
                let sample = wash.sample(now);
                moving |= !sample.done;
                selected || !sample.done
            });
        }
        motion::request_frame(window, moving);
    }
}

// W9-THEME: all colours come from `theme()` (see theme.rs).

// ──────────────────────────────────────────────────────────────
// Section keys (static strings used in SidebarState::collapsed)
// ──────────────────────────────────────────────────────────────

pub const SECTION_LOCAL: &str = "local";
pub const SECTION_REMOTE: &str = "remote";
pub const SECTION_TAGS: &str = "tags";
pub const SECTION_WORKTREES: &str = "worktrees";
pub const SECTION_STASHES: &str = "stashes";

/// Build hover help showing the full (untruncated) name.
/// Row labels are single-line + ellipsized, so the tooltip is how the user
/// reads a name that doesn't fit the sidebar width.
pub(super) fn name_tooltip(
    full: SharedString,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView + 'static {
    move |window, cx| Tooltip::new(full.clone()).build(window, cx)
}

// ──────────────────────────────────────────────────────────────
// PERF-SIDEBAR-VIRT: flat row model for `uniform_list`
// ──────────────────────────────────────────────────────────────
//
// Flatten all sections once when the input fingerprint changes; store five
// contiguous ranges into that Vec on `SidebarState`. Each Graph pane draws only
// its visible leaf rows via its own `uniform_list` and the common row renderer.
// Moving a divider or scrolling a pane never regroups thousands of refs.

/// One flattened, virtualized sidebar row. A section header starts each pane's
/// contiguous range and is drawn outside its leaf list so it stays pinned.
/// Every virtualized leaf in a pane has that pane's uniform slot height.
#[derive(Debug, Clone)]
pub enum SidebarRow {
    /// A top-level pane header. `section` is the static collapse key.
    SectionHeader {
        section: &'static str,
        title: Msg,
        count: usize,
        collapsed: bool,
    },
    /// A `/`-prefix group header inside LOCAL BRANCHES (collapse key
    /// `local:<prefix>`).
    LocalGroupHeader {
        key: String,
        prefix: String,
        count: usize,
        collapsed: bool,
    },
    /// A local branch leaf. `display_label` is what shows (prefix-stripped for
    /// grouped leaves); `name` is the full branch name used for handlers/id.
    LocalBranchLeaf {
        name: String,
        display_label: String,
        is_head: bool,
        indented: bool,
    },
    /// REMOTE BRANCHES level-1 header (a remote name). Collapse key
    /// `remote:<remote>`.
    RemoteHeader {
        key: String,
        remote: String,
        count: usize,
        collapsed: bool,
    },
    /// REMOTE BRANCHES level-2 sub-group header. Collapse key
    /// `remote:<remote>:<prefix>`.
    RemoteSubGroup {
        key: String,
        /// Owning remote, retained separately from the namespaced collapse key.
        remote: String,
        prefix: String,
        count: usize,
        collapsed: bool,
    },
    /// A remote branch leaf. `display` is the full `origin/…` name (used for
    /// jump/tooltip/id/drag); `display_label` is the prefix-stripped label;
    /// `depth` drives indentation (1 = direct under remote, 2 = under
    /// sub-group).
    RemoteLeaf {
        display: String,
        display_label: String,
        target: CommitId,
        depth: u8,
    },
    /// A tag leaf.
    Tag { name: String, target: CommitId },
    /// A worktree leaf.
    Worktree {
        name: String,
        /// Checked-out local branch, or `None` for a detached worktree.
        branch: Option<String>,
        /// The working-tree path itself. `path_label` is display text — lossy
        /// for non-UTF-8 paths and control-byte sanitized before rendering — so
        /// the menu's path actions use this instead of parsing the label back.
        path: std::path::PathBuf,
        path_label: String,
        is_current: bool,
        locked: bool,
        /// First port of the worktree's stored block, shown as
        /// `localhost:<port>` (#855). `None` when it has none.
        port: Option<u16>,
    },
    /// A stash leaf. `target` is the stash commit, which names the entry
    /// while its index shifts (a new stash pushes the others down).
    Stash {
        index: usize,
        message: String,
        target: CommitId,
    },
}

// ──────────────────────────────────────────────────────────────
// Per-row builders (called from the `uniform_list` processor)
// ──────────────────────────────────────────────────────────────

/// Dispatch a single flat row to its renderer. Reads live data from `this`
/// (upstream info, commit index) so handlers see current state, mirroring the
/// commit-list per-row builders.
pub(super) fn build_sidebar_row(
    this: &KagiApp,
    row: &SidebarRow,
    now_secs: i64,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    build_sidebar_row_presentation(this, row, now_secs, true, cx)
}

/// Ghosts reuse the real row's content but install no hit targets, focus or AX.
pub(super) fn build_sidebar_row_presentation(
    this: &KagiApp,
    row: &SidebarRow,
    now_secs: i64,
    interactive: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    match row {
        SidebarRow::SectionHeader {
            section,
            title,
            count,
            collapsed,
        } => build_section_header(section, *title, *count, *collapsed, cx),
        SidebarRow::LocalGroupHeader { .. }
        | SidebarRow::RemoteHeader { .. }
        | SidebarRow::RemoteSubGroup { .. } => build_group_row(row, interactive, cx),
        SidebarRow::LocalBranchLeaf {
            name,
            display_label,
            is_head,
            indented,
        } => build_local_branch_leaf(
            this,
            name,
            *is_head,
            display_label,
            *indented,
            now_secs,
            interactive,
            cx,
        ),
        SidebarRow::RemoteLeaf {
            display,
            display_label,
            target,
            depth,
        } => build_remote_leaf(
            this,
            display,
            display_label,
            target,
            *depth,
            now_secs,
            interactive,
            cx,
        ),
        SidebarRow::Tag { name, target } => {
            build_tag_row(this, name, target.clone(), interactive, cx)
        }
        SidebarRow::Worktree {
            name,
            branch,
            path,
            path_label,
            is_current,
            locked,
            port,
        } => super::sidebar_worktree_row::build_worktree_row(
            super::sidebar_worktree_row::WorktreeRowFacts {
                name,
                branch: branch.as_deref(),
                path,
                path_label,
                is_current: *is_current,
                locked: *locked,
                port: *port,
            },
            this,
            interactive,
            cx,
        ),
        SidebarRow::Stash { index, message, .. } => {
            build_stash_row(*index, message, interactive, cx)
        }
    }
}

/// Bound the root flex width and give the suffix a preferential shrink share.
fn branch_row_meta(
    row: gpui::Stateful<gpui::Div>,
    age: std::borrow::Cow<'static, str>,
    upstream: Option<SharedString>,
    pr: Option<(SharedString, u32)>,
) -> gpui::Stateful<gpui::Div> {
    row.w_full()
        .child(
            div()
                .flex_shrink(1000.)
                .min_w(px(0.))
                .truncate()
                .text_size(theme::scaled_px(SIDEBAR_AUX_TEXT))
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!("  \u{00b7} {age}"))),
        )
        .when_some(upstream, |el, text| {
            el.child(
                div()
                    .flex_shrink_0()
                    .ml_2()
                    .text_size(theme::scaled_px(SIDEBAR_AUX_TEXT))
                    .text_color(rgb(theme().text_sub))
                    .child(text),
            )
        })
        .when_some(pr, |el, (text, color)| {
            el.child(
                div()
                    .flex_shrink_0()
                    .ml_2()
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme().selected))
                    .text_size(theme::scaled_px(SIDEBAR_AUX_TEXT))
                    .text_color(rgb(color))
                    .child(text),
            )
        })
}

/// A local branch leaf. HEAD is a merge drop target that only jumps on click;
/// a non-HEAD row drags, checks out on double click, and carries the delete.
#[allow(clippy::too_many_arguments)] // Existing leaf facts plus inert-ghost presentation.
fn build_local_branch_leaf(
    this: &KagiApp,
    branch_name: &str,
    is_head: bool,
    display_label: &str,
    indented: bool,
    now_secs: i64,
    interactive: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let upstream_label: Option<SharedString> = this
        .view()
        .branch_upstream_info
        .get(branch_name)
        .and_then(|u| match u.counts {
            // #355: a skipped count is unknown — "—", never a hidden 0/0.
            None => Some(SharedString::from("\u{2014}")),
            Some(c) if c.ahead > 0 || c.behind > 0 => Some(SharedString::from(format!(
                "\u{2191}{} \u{2193}{}",
                c.ahead, c.behind
            ))),
            Some(_) => None,
        });

    // Show this session's open PR alongside its branch.
    let pr_badge: Option<(SharedString, u32)> = this
        .ui()
        .github_prs
        .iter()
        .find(|p| p.head == branch_name)
        .map(|p| {
            use kagi_domain::github::CiState;
            let (glyph, color) = match p.ci {
                CiState::Success => ("\u{2713}", theme().color_success),
                CiState::Failure => ("\u{2717}", theme().color_blocker),
                CiState::Pending => ("\u{25CF}", theme().color_warning),
                CiState::None => ("", theme().text_muted),
            };
            (
                SharedString::from(format!("#{} {}", p.number, glyph)),
                color,
            )
        });

    // issue #356: branch names are remote-origin — neutralize control bytes in
    // the visible label (the raw `branch_name` operand stays untouched).
    let display_label = kagi_domain::text_safety::sanitize_control_bytes(display_label);
    let view = this.view();
    let (age, committed) = view.tip_labels(view.branch_targets.get(branch_name), now_secs);
    let label = SharedString::from(if is_head {
        format!("\u{2713} {display_label}")
    } else {
        display_label
    });
    let text_color = if is_head {
        theme().color_success
    } else {
        theme().text_main
    };
    let full_name = SharedString::from(format!("{branch_name}\n{committed}"));
    let left_pad = if indented {
        theme::scaled_px(28.)
    } else {
        theme::scaled_px(12.)
    };

    let row = if is_head {
        let branch_for_click = branch_name.to_string();
        let branch_for_menu = branch_name.to_string();
        let head_click = cx.listener(move |this: &mut KagiApp, _e: &gpui::ClickEvent, _w, cx| {
            this.jump_to_branch(&branch_for_click);
            cx.notify();
        });
        let menu_click = cx.listener(
            move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
                this.open_local_branch_menu(branch_for_menu.clone(), event.position);
                cx.stop_propagation();
                cx.notify();
            },
        );
        let drop_handler = cx.listener(
            move |this: &mut KagiApp, payload: &BranchDrag, _window, cx| {
                this.start_merge_from_drag(payload.name.clone(), cx);
                cx.notify();
            },
        );
        div()
            .id(SharedString::from(format!(
                "sidebar-branch-{}",
                branch_name
            )))
            .h(theme::scaled_px(SIDEBAR_ROW_H))
            .flex()
            .flex_row()
            .items_center()
            .pl(left_pad)
            .pr_3()
            .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
            .line_height(gpui::relative(1.2))
            .text_color(rgb(text_color))
            .overflow_hidden()
            .when(interactive, |el| {
                el.on_click(head_click)
                    .on_mouse_down(gpui::MouseButton::Right, menu_click)
                    .map(|el| super::branch_drag::feedback(el, branch_name.to_string(), cx))
                    .on_drop::<BranchDrag>(drop_handler)
                    .hover(|style| style.bg(rgb(theme().surface)))
            })
            .child(
                div()
                    .relative()
                    .flex_auto()
                    .truncate()
                    .line_height(gpui::relative(1.2))
                    .child(label)
                    .when(cfg!(feature = "gui-e2e"), |label| {
                        label.child(super::e2e::measure_inside(format!(
                            "sidebar-local-name-{branch_name}"
                        )))
                    }),
            )
            .map(|el| branch_row_meta(el, age, upstream_label, pr_badge))
            .map(|el| {
                if interactive {
                    el.hover_tooltip_builder(
                        SharedString::from(format!("sidebar-branch-{branch_name}-hover-help")),
                        name_tooltip(full_name),
                    )
                    .into_any_element()
                } else {
                    el.into_any_element()
                }
            })
    } else {
        let branch_for_dbl = branch_name.to_string();
        let branch_for_delete = branch_name.to_string();
        let branch_for_menu = branch_name.to_string();
        let branch_for_drag = branch_name.to_string();
        let click_handler = cx.listener(
            move |this: &mut KagiApp, event: &gpui::ClickEvent, _window, cx| {
                if event.click_count() >= 2 {
                    this.open_plan_modal(branch_for_dbl.clone(), cx);
                } else {
                    this.jump_to_branch(&branch_for_dbl);
                }
                cx.notify();
            },
        );
        let delete_handler = cx.listener(
            move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                this.open_delete_branch_modal(branch_for_delete.clone(), cx);
                cx.notify();
            },
        );
        let menu_click = cx.listener(
            move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
                this.open_local_branch_menu(branch_for_menu.clone(), event.position);
                cx.stop_propagation();
                cx.notify();
            },
        );
        // ADR-0144: a non-HEAD row is both a drag source and a drop target.
        // Dropping here merges into a branch that is not checked out — only
        // its ref moves, the working tree is untouched.
        let branch_for_drop = branch_name.to_string();
        let drop_handler = cx.listener(
            move |this: &mut KagiApp, payload: &BranchDrag, _window, cx| {
                this.start_merge_into_from_drag(payload.name.clone(), branch_for_drop.clone(), cx);
                cx.notify();
            },
        );
        div()
            .id(SharedString::from(format!(
                "sidebar-branch-{}",
                branch_name
            )))
            .h(theme::scaled_px(SIDEBAR_ROW_H))
            .flex()
            .flex_row()
            .items_center()
            .pl(left_pad)
            .pr_3()
            .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
            .line_height(gpui::relative(1.2))
            .text_color(rgb(text_color))
            .overflow_hidden()
            .when(interactive, |el| {
                el.on_click(click_handler)
                    .on_mouse_down(gpui::MouseButton::Right, menu_click)
                    .map(|el| super::branch_drag::feedback(el, branch_name.to_string(), cx))
                    .on_drop::<BranchDrag>(drop_handler)
                    .on_drag(
                        BranchDrag {
                            name: branch_for_drag.clone(),
                        },
                        move |drag: &BranchDrag, _pos, _window, cx| {
                            let name = SharedString::from(drag.name.clone());
                            cx.new(|_| BranchDragGhost { name })
                        },
                    )
                    .hover(|style| style.bg(rgb(theme().surface)))
            })
            .child(
                div()
                    .relative()
                    .flex_auto()
                    .truncate()
                    .line_height(gpui::relative(1.2))
                    .child(label)
                    .when(cfg!(feature = "gui-e2e"), |label| {
                        label.child(super::e2e::measure_inside(format!(
                            "sidebar-local-name-{branch_name}"
                        )))
                    }),
            )
            .map(|el| branch_row_meta(el, age, upstream_label, pr_badge))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "sidebar-delete-{}",
                        branch_name
                    )))
                    .flex_shrink_0()
                    .ml_1()
                    .px_1()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .when(interactive, |el| {
                        el.on_click(delete_handler)
                            .hover(|s| s.text_color(rgb(theme().color_blocker)))
                    })
                    .child(SharedString::from("\u{00d7}")),
            )
            .map(|el| {
                if interactive {
                    el.hover_tooltip_builder(
                        SharedString::from(format!("sidebar-branch-{branch_name}-hover-help")),
                        name_tooltip(full_name),
                    )
                    .into_any_element()
                } else {
                    el.into_any_element()
                }
            })
    };
    #[cfg(feature = "gui-e2e")]
    {
        super::e2e::measure_control(format!("sidebar-local-{branch_name}"), row)
    }
    #[cfg(not(feature = "gui-e2e"))]
    {
        row
    }
}

/// A remote branch leaf: a merge drag source, clickable while its tip is loaded.
#[allow(clippy::too_many_arguments)] // Share the real leaf renderer with noninteractive ghosts.
fn build_remote_leaf(
    this: &KagiApp,
    display: &str,
    display_label: &str,
    rb_target: &CommitId,
    depth: u8,
    now_secs: i64,
    interactive: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let can_jump = this.view().commit_row_index.contains_key(rb_target);
    let (age, committed) = this.view().tip_labels(Some(rb_target), now_secs);
    // issue #414: remote branch names are the most literally remote-derived
    // strings in the sidebar. Neutralize control bytes in the *displayed* label
    // and tooltip; the raw `display` operand (drag/menu/id) stays untouched.
    let safe_display = kagi_domain::text_safety::sanitize_control_bytes(display);
    let full_name = SharedString::from(format!("{safe_display}\n{committed}"));
    let label = kagi_domain::text_safety::sanitize_control_bytes(display_label);
    let drag_name = display.to_string();
    let left_pad = match depth {
        0 => theme::scaled_px(12.),
        1 => theme::scaled_px(28.),
        _ => theme::scaled_px(44.),
    };
    let display_for_menu = display.to_string();
    let target_for_menu = rb_target.clone();
    let menu_click = cx.listener(
        move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
            this.open_remote_branch_menu(
                display_for_menu.clone(),
                target_for_menu.clone(),
                event.position,
            );
            cx.stop_propagation();
            cx.notify();
        },
    );

    let base = div()
        .id(SharedString::from(format!("sidebar-remote-{}", display)))
        .relative()
        .when(cfg!(feature = "gui-e2e"), |row| {
            row.child(super::e2e::measure_inside(format!(
                "sidebar-remote-{display}"
            )))
        })
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .pl(left_pad)
        .pr_3()
        .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
        .line_height(gpui::relative(1.2))
        .text_color(rgb(theme().color_remote))
        .overflow_hidden()
        .when(interactive, |el| {
            el.on_mouse_down(gpui::MouseButton::Right, menu_click)
                .cursor_grab()
                .on_drag(
                    BranchDrag {
                        name: drag_name.clone(),
                    },
                    move |drag: &BranchDrag, _pos, _window, cx| {
                        let name = SharedString::from(drag.name.clone());
                        cx.new(|_| BranchDragGhost { name })
                    },
                )
                .hover(|style| style.bg(rgb(theme().surface)))
        })
        .child(
            div()
                .relative()
                .flex_auto()
                .truncate()
                .line_height(gpui::relative(1.2))
                .child(label)
                .when(cfg!(feature = "gui-e2e"), |label| {
                    label.child(super::e2e::measure_inside(format!(
                        "sidebar-remote-name-{display}"
                    )))
                }),
        )
        .map(|el| branch_row_meta(el, age, None, None));

    let base = if can_jump && interactive {
        let target_for_jump = rb_target.clone();
        let click_handler = cx.listener(
            move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                this.jump_to_commit(&target_for_jump);
                cx.notify();
            },
        );
        base.on_click(click_handler)
    } else {
        base
    };
    if interactive {
        base.hover_tooltip_builder(
            SharedString::from(format!("sidebar-remote-{display}-hover-help")),
            name_tooltip(full_name),
        )
        .into_any_element()
    } else {
        base.into_any_element()
    }
}

// ──────────────────────────────────────────────────────────────
// W13-BRANCHTREE: unit tests for the pure grouping helpers
// ──────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "sidebar_tests.rs"]
mod tests;
