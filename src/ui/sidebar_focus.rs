//! The Graph sidebar's rows from the keyboard (#981).
//!
//! Each of the five panes is its own tree and its own Tab stop: the row the
//! keyboard last reached while it is drawn, else the first row on screen
//! (`keyboard_nav::RowFocus` over the pane's `uniform_list`). ↑/↓ stay inside
//! the pane. A pane that offers no drawn row — collapsed, empty, emptied by
//! the filter, or too short to draw one — has its header as its Tab stop
//! instead (#981 review), and on a collapsed one Enter / Space open the pane
//! and move to its first row.
//!
//! Enter / Space press a row: a branch opens its checkout plan (the current
//! branch: jumps to it), a worktree opens or closes its inspection card (the
//! hover card, anchored to the row; Escape closes it too), a group opens or
//! closes, a remote branch or tag jumps to its commit, a stash opens its
//! peek. The rest is what a click does; a double-click's checkout is Enter
//! because a single click on a branch only jumps to it.
//!
//! A pointer click does not put the focus on a row: the window takes it, as
//! before, so ↑/↓ after clicking a branch still step the graph.

use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    deferred, div, point, prelude::*, px, AnchoredPositionMode, AnyElement, ClickEvent, Context,
    Div, FocusHandle, MouseButton, Stateful, Window,
};

use super::keyboard_nav::{FocusSlots, RowFocus, RowKeys, RowList, RowScroll, RING};
use super::sidebar::SidebarRow;
use super::KagiApp;

const PANES: usize = 5;

/// The WORKTREES pane (`sidebar_panes::SECTIONS[2]`), whose rows open the
/// keyboard's inspection card.
const WORKTREES: usize = 2;

/// The sidebar panes' keyboard state, kept across frames.
#[derive(Default)]
pub(crate) struct SidebarFocus {
    /// Each pane's rows as `(key, item)`, built with the sidebar's rows.
    keys: [RowKeys; PANES],
    rows: [RowFocus; PANES],
    /// This frame's rows, for `render_pane`.
    pub(super) lists: [Option<Rc<RowList>>; PANES],
    /// The items each pane's list drew when it was last on screen (kept
    /// while the panes are away, so the frame they come back on still has
    /// its rows' Tab stops — #981 review).
    pub(super) drawn: [Range<usize>; PANES],
    /// The items each pane's list was last asked to draw, kept across
    /// frames: a change asks for one more frame (#987 review).
    pub(super) seen: [Range<usize>; PANES],
    /// The headers: a collapsed pane's Tab stop.
    pub(super) headers: FocusSlots,
    /// A pane opened from its header by the keyboard: its first row takes
    /// the focus once its rows are built.
    open_first: [bool; PANES],
    /// The worktree whose inspection card the keyboard opened.
    pub(super) card: Option<PathBuf>,
    /// The panes were not drawn last frame.
    away: bool,
}

/// A row's key among its pane's rows; `None` for a pane header, which is not
/// one of the list's rows.
fn row_key(row: &SidebarRow) -> Option<String> {
    Some(match row {
        SidebarRow::SectionHeader { .. } => return None,
        SidebarRow::LocalGroupHeader { key, .. }
        | SidebarRow::RemoteHeader { key, .. }
        | SidebarRow::RemoteSubGroup { key, .. } => format!("group:{key}"),
        SidebarRow::LocalBranchLeaf { name, .. } => format!("branch:{name}"),
        SidebarRow::RemoteLeaf { display, .. } => format!("remote:{display}"),
        SidebarRow::Tag { name, .. } => format!("tag:{name}"),
        SidebarRow::Worktree { path, .. } => worktree_key(path),
        // The stash commit, not `stash@{N}`: a new stash shifts every index,
        // and the focus must stay on its entry (#987 review).
        SidebarRow::Stash { target, .. } => format!("stash:{target}"),
    })
}

/// A worktree row's key, lossless (#987 review): its path as text when it is
/// UTF-8, else its bytes in hex under another prefix — two paths that
/// `Path::display` would print alike keep their own keys, and handles.
fn worktree_key(path: &std::path::Path) -> String {
    match path.to_str() {
        Some(text) => format!("worktree:{text}"),
        None => {
            let bytes = path.as_os_str().as_encoded_bytes();
            let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            format!("worktree-bytes:{hex}")
        }
    }
}

/// Whether `key` is the row key of the worktree at `path` (no allocation for
/// a UTF-8 path, which is every path the card is opened on in practice).
fn is_worktree_key(key: &str, path: &std::path::Path) -> bool {
    match path.to_str() {
        Some(text) => key.strip_prefix("worktree:") == Some(text),
        None => key == worktree_key(path),
    }
}

impl SidebarFocus {
    /// The sidebar's rows were rebuilt: each pane's rows, keyed, as the item
    /// they are in its list (the pane's rows after its header).
    pub(super) fn set_keys(&mut self, rows: &[SidebarRow], ranges: &[Range<usize>; PANES]) {
        for (pane, range) in ranges.iter().enumerate() {
            let body = rows.get(range.start + 1..range.end).unwrap_or_default();
            // A key seen twice in a pane (one commit stored as two stashes)
            // gets its place appended, so every row keeps its own handle.
            let mut seen = std::collections::HashSet::new();
            self.keys[pane] = body
                .iter()
                .enumerate()
                .filter_map(|(item, row)| row_key(row).map(|key| (key, item)))
                .map(|(key, item)| {
                    if seen.insert(key.clone()) {
                        (key, item)
                    } else {
                        (format!("{key}#{item}"), item)
                    }
                })
                .collect();
        }
    }
}

impl KagiApp {
    /// Each workspace frame, as the body is laid out: every pane's Tab stop
    /// and rows. `front` is whether this frame draws the panes — the sidebar
    /// is shown (or still closing) and on its Graph page. Decided from this
    /// frame's layout, not from what the last frame drew: the frame the
    /// sidebar closes on may be the last one drawn (#981 review), so a focus
    /// left on a row then would stay on nothing. Out of front, the focus
    /// goes to the window; in front, a row holding it that is not on screen
    /// hands it to the pane's header, as a row scrolled away does.
    pub(super) fn sync_sidebar_focus(
        &mut self,
        front: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !front {
            self.yield_sidebar_focus(window, cx);
            return;
        }
        // Back on screen: this frame picks the Tab stops from what the panes
        // drew when last shown; the next one, asked for here, from what this
        // one draws — nothing else may draw it (#981 review).
        if std::mem::take(&mut self.sidebar.focus.away) {
            window.request_animation_frame();
        }
        for pane in 0..PANES {
            let header = self.sidebar.focus.headers.get(pane, cx);
            let drawn = std::mem::take(&mut self.sidebar.focus.drawn[pane]);
            let scroll = RowScroll::Uniform(self.sidebar.scroll_handles[pane].clone(), drawn);
            let focus = &mut self.sidebar.focus;
            let list =
                focus.rows[pane].rows(focus.keys[pane].clone(), &scroll, Some(&header), window, cx);
            if focus.open_first[pane] {
                // A pane opened from its header: its first row takes the
                // focus once a row is drawn. Until then (the rebuild frame,
                // or a pane too short to draw one) the header keeps it
                // (#987 review); moving the focus off the header drops it.
                if !header.is_focused(window) {
                    focus.open_first[pane] = false;
                } else if list.has_stop() {
                    focus.open_first[pane] = false;
                    focus.rows[pane].focus_first(&scroll, window, cx);
                }
            } else if header.is_focused(window) && list.has_stop() {
                // The header stood in while the pane had no drawn row; now
                // one is drawn (a filter cleared, a refresh, a resize) the
                // header is no Tab stop, and its focus moves to the row that
                // is (#987 review).
                list.focus_stop(window, cx);
            }
            focus.lists[pane] = Some(Rc::new(list));
        }
        // The keyboard's inspection card lives while its row has the focus.
        if let Some(path) = &self.sidebar.focus.card {
            let held = self.sidebar.focus.rows[WORKTREES]
                .focused(window)
                .is_some_and(|key| is_worktree_key(key, path));
            if !held {
                self.sidebar.focus.card = None;
            }
        }
    }

    /// The panes are not drawn this frame (Home in front, Conflict Mode, the
    /// sidebar hidden, another page): a focus on one of their rows or
    /// headers goes to the window. The rows, and what the panes drew when
    /// last shown, are remembered for when they come back.
    pub(super) fn yield_sidebar_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.root_focus.clone();
        for pane in 0..PANES {
            self.sidebar.focus.rows[pane].yield_focus(root.as_ref(), window, cx);
            let header = self.sidebar.focus.headers.get(pane, cx);
            if header.is_focused(window) {
                if let Some(root) = &root {
                    root.focus(window, cx);
                }
            }
            self.sidebar.focus.lists[pane] = None;
        }
        self.sidebar.focus.away = true;
        self.sidebar.focus.card = None;
    }

    /// Enter / Space on a collapsed pane's header: open it, and its first row
    /// takes the focus once its rows are built.
    fn open_sidebar_pane(&mut self, pane: usize, cx: &mut Context<Self>) {
        let section = super::sidebar_panes::SECTIONS[pane];
        if self.sidebar.collapsed.contains(section) {
            self.sidebar.toggle_section(section);
            self.sidebar.focus.open_first[pane] = true;
            cx.notify();
        }
    }

    /// Enter / Space on a row: its main action (see the module docs).
    fn activate_sidebar_row(&mut self, row: &SidebarRow, cx: &mut Context<Self>) {
        match row {
            SidebarRow::SectionHeader { .. } => {}
            SidebarRow::LocalGroupHeader { key, .. }
            | SidebarRow::RemoteHeader { key, .. }
            | SidebarRow::RemoteSubGroup { key, .. } => {
                self.with_ui(|ui| ui.toggle_branch_group(key));
            }
            SidebarRow::LocalBranchLeaf {
                name,
                is_head: true,
                ..
            } => self.jump_to_branch(name),
            SidebarRow::LocalBranchLeaf { name, .. } => {
                self.open_plan_modal(name.clone());
                // The plan's Enter / Escape run through the root (#817);
                // left on the row, Enter would press the row again.
                if self.active_modal.is_some() {
                    self.focus_root_for_modal();
                }
            }
            SidebarRow::RemoteLeaf { target, .. } | SidebarRow::Tag { target, .. } => {
                if self.view().commit_row_index.contains_key(target) {
                    self.jump_to_commit(target);
                }
            }
            SidebarRow::Stash { index, .. } => self.open_stash_peek(*index, cx),
            // An SSH tab's worktree has no card (its row carries a tooltip).
            SidebarRow::Worktree { path, .. } if self.remote_view.is_none() => {
                if self.sidebar.focus.card.as_ref() == Some(path) {
                    self.sidebar.focus.card = None;
                } else {
                    self.sidebar.focus.card = Some(path.clone());
                    self.select_worktree_inspection(path.clone(), cx);
                }
            }
            SidebarRow::Worktree { .. } => {}
        }
        cx.notify();
    }
}

/// A pane header: the pane's Tab stop while the pane offers no drawn row to
/// Tab to (collapsed, empty, or too short to draw one), so every pane keeps
/// one; on a collapsed pane Enter / Space open it. `header` is the drawn
/// heading, which keeps its own click.
pub(super) fn header(
    app: &KagiApp,
    pane: usize,
    collapsed: bool,
    header: AnyElement,
    cx: &mut Context<KagiApp>,
) -> Stateful<Div> {
    let focus = app.sidebar.focus.headers.get(pane, cx);
    // The header stands in while the pane offers no row to Tab to: collapsed,
    // empty, filtered empty, or too short to draw one (#981 review).
    let stop = collapsed
        || !app.sidebar.focus.lists[pane]
            .as_deref()
            .is_some_and(RowList::has_stop);
    super::keyboard_nav::focusable(
        div()
            .id(("sidebar-pane-header", pane))
            .track_focus(&focus.tab_index(0).tab_stop(stop)),
    )
    .on_click(cx.listener(move |this, event: &ClickEvent, _window, cx| {
        if event.is_keyboard() {
            this.open_sidebar_pane(pane, cx);
        }
    }))
    .child(under_ring(header, app.root_focus.clone()))
}

/// Row `at` of `list`: its Tab stop, ↑/↓, and Enter / Space for its main
/// action. `el` is the row's slot; `row` what it draws.
pub(super) fn row(
    app: &KagiApp,
    list: &RowList,
    at: usize,
    el: Stateful<Div>,
    row: SidebarRow,
    drawn: AnyElement,
    cx: &mut Context<KagiApp>,
) -> Stateful<Div> {
    let card = match &row {
        SidebarRow::Worktree {
            name,
            branch,
            path,
            port,
            ..
        } if app.sidebar.focus.card.as_ref() == Some(path) => Some((
            path.clone(),
            super::sidebar_worktree_row::keyboard_card(
                app,
                path.clone(),
                name,
                branch.as_deref(),
                *port,
                cx,
            ),
        )),
        _ => None,
    };
    let el = list
        .row(at, el)
        .child(under_ring(drawn, app.root_focus.clone()))
        .on_click(cx.listener(move |this, event: &ClickEvent, _window, cx| {
            if event.is_keyboard() {
                this.activate_sidebar_row(&row, cx);
            }
        }));
    match card {
        Some((path, card)) => {
            let width = super::theme::scaled_px(app.sidebar.width);
            // Escape is bound to an action (`CloseMainDiff`), which runs
            // before key listeners: the row takes it while its card is open.
            el.on_action(
                cx.listener(move |this, _: &super::CloseMainDiff, _window, cx| {
                    if this.sidebar.focus.card.as_ref() == Some(&path) {
                        this.sidebar.focus.card = None;
                        cx.notify();
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .child(deferred(
                gpui::anchored()
                    .position_mode(AnchoredPositionMode::Local)
                    .position(point(width, px(0.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(card),
            ))
        }
        _ => el,
    }
}

/// What a ringed slot draws: pulled out under the ring by its width, so the
/// row looks as it did before it had one (the ring paints over its edge).
/// A pointer press — left or right — gives the window the focus, as before,
/// not the slot: a right-click's menu does not leave Enter / Space to the
/// row behind it (#987 review). The right press is taken on the way down,
/// as the row's own right-click (its menu) stops it on the way up.
fn under_ring(el: AnyElement, root: Option<FocusHandle>) -> Div {
    let to_root = move |window: &mut Window, cx: &mut gpui::App| {
        if let Some(root) = &root {
            window.focus(root, cx);
        }
        window.prevent_default();
    };
    let right = to_root.clone();
    div()
        .m(-px(RING))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| to_root(window, cx))
        .capture_any_mouse_down(move |event, window, cx| {
            if event.button == MouseButton::Right {
                right(window, cx);
            }
        })
        .child(el)
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// The pane and key of the sidebar row holding the focus, if any.
    pub fn sidebar_row_focused_for_e2e(&self, window: &Window) -> Option<(usize, String)> {
        (0..PANES).find_map(|pane| {
            self.sidebar.focus.rows[pane]
                .focused(window)
                .map(|key| (pane, key.to_string()))
        })
    }

    /// Focus the sidebar row `key` of `pane`.
    pub fn focus_sidebar_row_for_e2e(
        &self,
        pane: usize,
        key: &str,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        self.sidebar.focus.rows[pane].focus(key, window, cx);
    }

    /// Which pane header holds the focus, if any.
    pub fn sidebar_header_focused_for_e2e(&self, window: &Window) -> Option<usize> {
        self.sidebar.focus.headers.focused(window)
    }

    /// `pane`'s row keys, in drawing order.
    pub fn sidebar_row_keys_for_e2e(&self, pane: usize) -> Vec<String> {
        self.sidebar.focus.keys[pane]
            .iter()
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// The worktree whose inspection card the keyboard opened.
    pub fn sidebar_keyboard_card_for_e2e(&self) -> Option<PathBuf> {
        self.sidebar.focus.card.clone()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    use super::{is_worktree_key, worktree_key};

    /// Two worktree paths that `Path::display` prints alike (each invalid
    /// byte becomes U+FFFD) keep their own row keys, so their rows keep their
    /// own focus handles; each key still names its own path (#987 review).
    #[test]
    fn worktree_keys_tell_apart_paths_that_display_alike() {
        let a = Path::new(OsStr::from_bytes(b"/wt/\xff"));
        let b = Path::new(OsStr::from_bytes(b"/wt/\xfe"));
        assert_eq!(a.display().to_string(), b.display().to_string());
        assert_ne!(worktree_key(a), worktree_key(b));
        assert!(is_worktree_key(&worktree_key(a), a));
        assert!(!is_worktree_key(&worktree_key(a), b));
        let utf8 = Path::new("/wt/one");
        assert_eq!(worktree_key(utf8), "worktree:/wt/one");
        assert!(is_worktree_key("worktree:/wt/one", utf8));
    }
}
