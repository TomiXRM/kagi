//! Whole-file hunk RowList, with horizontal adoption controls sharing navigation.
use super::button_style::{ButtonRole, KagiButton};
use super::{
    conflict_view::ConflictView,
    keyboard_nav::{self, RowFocus, RowKeys, RowList, RowScroll},
};
use gpui::{
    actions, prelude::*, App, Context, FocusHandle, KeyBinding, Role, SharedString, Window,
};
use gpui_component::Disableable as _;
use gpui_component::Sizable;
use kagi_git::resolution::{HunkModel, Region, SelectionSide, TriState};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

actions!(
    conflict_resolver,
    [
        PreviousControl,
        NextControl,
        PreviousTabControl,
        NextTabControl,
        ReturnToHeader
    ]
);
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", PreviousControl, Some("KagiConflictResolver")),
        KeyBinding::new("right", NextControl, Some("KagiConflictResolver")),
        KeyBinding::new(
            "shift-tab",
            PreviousTabControl,
            Some("KagiConflictResolver"),
        ),
        KeyBinding::new("tab", NextTabControl, Some("KagiConflictResolver")),
        KeyBinding::new("up", ReturnToHeader, Some("KagiConflictResolverControl")),
    ]);
}
pub(crate) fn center_hunk(scroll: &gpui::UniformListScrollHandle, action_row: usize) {
    let row_height = super::theme::scaled_px(24.);
    let height = scroll.0.borrow().base_handle.bounds().size.height;
    // Center the first code line when both it and its action row fit.
    // Compact panes instead put the pair at the top, keeping both reachable.
    let (row, strategy) = if height < row_height * 3. {
        (action_row, gpui::ScrollStrategy::Top)
    } else {
        (action_row + 1, gpui::ScrollStrategy::Center)
    };
    scroll.scroll_to_item_strict(row, strategy);
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum RegionShape {
    Context(usize),
    Hunk(usize, usize, bool),
}
impl RegionShape {
    fn of(region: &Region) -> Self {
        match region {
            Region::Passthrough(lines) => Self::Context(lines.len()),
            Region::Hunk(hunk) => {
                Self::Hunk(hunk.current.len(), hunk.incoming.len(), hunk.is_resolved())
            }
        }
    }
}
#[derive(Default)]
pub struct ResolverKeyboard {
    path: PathBuf,
    structure: Vec<RegionShape>,
    pub side_names: [String; 2],
    rows: RowKeys,
    focus: RowFocus,
    list: Option<RowList>,
    controls: Vec<Vec<(String, FocusHandle, usize)>>,
    selected_hunk: Option<usize>,
    tab_hunk: usize,
    viewport: Option<(gpui::Size<gpui::Pixels>, gpui::Pixels, gpui::Pixels)>,
    pub drawn: std::ops::Range<usize>,
}
impl ResolverKeyboard {
    pub(crate) fn prepare(
        &mut self,
        path: Option<(&Path, &HunkModel)>,
        selected_hunk: usize,
        scroll: &gpui::UniformListScrollHandle,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some((path, model)) = path else {
            self.release(window, cx);
            return;
        };
        let focused_hunk = self.list.as_ref().and_then(|list| {
            (0..list.len()).find(|&at| {
                list.focus_handle(at).is_focused(window)
                    || self.controls[at]
                        .iter()
                        .any(|(_, handle, _)| handle.is_focused(window))
            })
        });
        let focused_id = focused_hunk.and_then(|hunk| {
            self.controls[hunk]
                .iter()
                .find(|(_, handle, _)| handle.is_focused(window))
                .map(|(id, _, _)| id.clone())
                .or_else(|| self.rows.get(hunk).map(|(id, _)| id.clone()))
        });
        let mut refocus = false;
        let mut rebuilt = false;
        if self.path != path
            || !self
                .structure
                .iter()
                .copied()
                .eq(model.regions.iter().map(RegionShape::of))
        {
            refocus = self.release(window, cx);
            rebuilt = true;
            self.path = path.into();
            self.structure
                .extend(model.regions.iter().map(RegionShape::of));
            let mut rows = Vec::new();
            let mut row = 0;
            let mut hunk_index = 0;
            for region in &model.regions {
                match region {
                    Region::Passthrough(lines) => row += lines.len(),
                    Region::Hunk(hunk) => {
                        let n = hunk_index;
                        if n == selected_hunk {
                            center_hunk(scroll, row);
                        }
                        let actions: &[&str] = if hunk.is_resolved() {
                            &["current-change", "incoming-change"]
                        } else {
                            &[
                                "current-use",
                                "current-both",
                                "incoming-use",
                                "incoming-both",
                            ]
                        };
                        rows.push((format!("conflict-hunk-{n}-{}", actions[0]), row));
                        let mut controls = actions
                            .iter()
                            .skip(1)
                            .map(|action| {
                                (
                                    format!("conflict-hunk-{n}-{action}"),
                                    cx.focus_handle(),
                                    row,
                                )
                            })
                            .collect::<Vec<_>>();
                        for (side, lines) in
                            [("current", &hunk.current), ("incoming", &hunk.incoming)]
                        {
                            for line in 0..lines.len() {
                                controls.push((
                                    format!("resolver-hunk-{n}-{side}-line-{line}"),
                                    cx.focus_handle(),
                                    row + 1 + line,
                                ));
                            }
                        }
                        self.controls.push(controls);
                        row += 1 + hunk.current.len().max(hunk.incoming.len()).max(1);
                        hunk_index += 1;
                    }
                }
            }
            self.rows = rows.into();
        }
        // Selection chooses the entry point only when it changes. Arrow focus
        // otherwise roves independently, including through unselected hunks.
        if rebuilt {
            self.tab_hunk = focused_hunk
                .unwrap_or(selected_hunk)
                .min(self.rows.len().saturating_sub(1));
        } else if self.selected_hunk != Some(selected_hunk) {
            self.tab_hunk = selected_hunk.min(self.rows.len().saturating_sub(1));
        } else if let Some(list) = &self.list {
            if let Some(at) = (0..list.len()).find(|&at| {
                list.focus_handle(at).is_focused(window)
                    || self.controls[at]
                        .iter()
                        .any(|(_, handle, _)| handle.is_focused(window))
            }) {
                self.tab_hunk = at;
            }
        }
        self.selected_hunk = Some(selected_hunk);
        self.list = Some(self.focus.rows(
            self.rows.clone(),
            &RowScroll::Uniform(scroll.clone(), self.drawn.clone()),
            None,
            window,
            cx,
        ));
        if refocus {
            if let Some(list) = &self.list {
                if let Some((_, row)) = self.rows.get(self.tab_hunk) {
                    list.reveal_entry(*row);
                    let id = focused_id.as_deref().unwrap_or("");
                    let side = if id.contains("-incoming-") {
                        "incoming"
                    } else {
                        "current"
                    };
                    let action = if self.rows[self.tab_hunk].0.ends_with("-change") {
                        "change"
                    } else {
                        "use"
                    };
                    let replacement = format!("conflict-hunk-{}-{side}-{action}", self.tab_hunk);
                    let target = if self.controls[self.tab_hunk]
                        .iter()
                        .any(|(key, _, _)| key == id)
                    {
                        id
                    } else {
                        &replacement
                    };
                    self.focus_control(self.tab_hunk, target, window, cx);
                }
            }
        }
        let viewport = (
            window.viewport_size(),
            super::theme::scaled_px(24.),
            scroll.0.borrow().base_handle.bounds().size.height,
        );
        if self.viewport != Some(viewport) {
            // A newly inserted action row must not push the first code line
            // below a compact viewport after a resize or zoom change.
            if let Some((_, row)) = self.rows.get(focused_hunk.unwrap_or(selected_hunk)) {
                center_hunk(scroll, *row);
            }
            self.viewport = Some(viewport);
        }
    }
    fn release(&mut self, window: &mut Window, cx: &mut App) -> bool {
        let focused = self
            .controls
            .iter()
            .flatten()
            .any(|(_, handle, _)| handle.is_focused(window))
            || self.focus.focused(window).is_some();
        if focused {
            window.blur();
        }
        self.focus.release(None, window, cx);
        self.path.clear();
        self.controls.clear();
        self.structure.clear();
        self.rows = Rc::default();
        self.list = None;
        self.drawn = 0..0;
        self.selected_hunk = None;
        self.tab_hunk = 0;
        self.viewport = None;
        focused
    }
    pub(crate) fn focus_hunk(&self, hunk: usize, window: &mut Window, cx: &mut App) {
        if let Some(list) = &self.list {
            list.focus_row(hunk, window, cx);
        }
    }
    pub(crate) fn focus_control(&self, hunk: usize, id: &str, window: &mut Window, cx: &mut App) {
        if let Some((_, handle, row)) = self
            .controls
            .get(hunk)
            .and_then(|controls| controls.iter().find(|(key, _, _)| key == id))
        {
            if let Some(list) = &self.list {
                list.reveal_entry(*row);
            }
            handle.focus(window, cx);
        } else {
            self.focus_hunk(hunk, window, cx);
        }
    }
    pub(crate) fn side_name(&self, side: SelectionSide) -> &str {
        &self.side_names[match side {
            SelectionSide::Current => 0,
            SelectionSide::Incoming => 1,
        }]
    }
    pub(crate) fn focus_line(
        &self,
        hunk: usize,
        side: SelectionSide,
        line: usize,
        window: &mut Window,
        cx: &mut App,
    ) {
        let side = match side {
            SelectionSide::Current => "current",
            SelectionSide::Incoming => "incoming",
        };
        let id = format!("resolver-hunk-{hunk}-{side}-line-{line}");
        if let Some((_, handle, row)) = self
            .controls
            .get(hunk)
            .and_then(|items| items.iter().find(|(key, _, _)| key == &id))
        {
            if let Some(list) = &self.list {
                list.reveal_entry(*row);
            }
            handle.focus(window, cx);
        }
    }

    fn neighbour_line(
        &self,
        hunk: usize,
        side: SelectionSide,
        line: usize,
        forward: bool,
    ) -> Option<(usize, usize)> {
        let hunks = self.structure.iter().filter_map(|shape| match shape {
            RegionShape::Hunk(current, incoming, _) => Some(match side {
                SelectionSide::Current => *current,
                SelectionSide::Incoming => *incoming,
            }),
            RegionShape::Context(_) => None,
        });
        if forward {
            for (index, count) in hunks.enumerate().skip(hunk) {
                let next = if index == hunk { line + 1 } else { 0 };
                if next < count {
                    return Some((index, next));
                }
            }
            None
        } else if line > 0 {
            Some((hunk, line - 1))
        } else {
            hunks
                .enumerate()
                .take(hunk)
                .filter_map(|(index, count)| count.checked_sub(1).map(|line| (index, line)))
                .last()
        }
    }
    fn move_control(
        &self,
        hunk: usize,
        id: &str,
        direction: isize,
        window: &mut Window,
        cx: &mut App,
    ) {
        let controls = &self.controls[hunk];
        let at = controls
            .iter()
            .position(|(key, _, _)| key == id)
            .map_or(0, |at| at + 1);
        let to = (at as isize + direction).clamp(0, controls.len() as isize) as usize;
        if to == 0 {
            if let Some(list) = &self.list {
                list.reveal_entry(self.rows[hunk].1);
                list.focus_row(hunk, window, cx);
            }
        } else {
            let (_, handle, row) = &controls[to - 1];
            if let Some(list) = &self.list {
                list.reveal_entry(*row);
            }
            handle.focus(window, cx);
        }
    }
}
fn state_description(state: TriState) -> (&'static str, SharedString) {
    use super::i18n::Msg;
    let (key, message) = match state {
        TriState::None => ("none", Msg::EditorSelectionNone),
        TriState::Partial => ("partial", Msg::EditorSelectionPartial),
        TriState::All => ("all", Msg::EditorSelectionAll),
    };
    (key, message.t().into())
}
/// The pointer control owns feedback; its enclosing roving owner handles
/// keyboard clicks. Both call the very same listener, never duplicate mutation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn adoption(
    id: String,
    name: String,
    state: TriState,
    content: &'static str,
    role: ButtonRole,
    background: u32,
    hunk: Option<usize>,
    enabled: bool,
    owner: &Rc<RefCell<ResolverKeyboard>>,
    activate: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<ConflictView>,
) -> gpui::Stateful<gpui::Div> {
    let activate = Rc::new(activate);
    let pointer = activate.clone();
    let (_state, description) = state_description(state);
    #[cfg(feature = "gui-e2e")]
    super::conflict_editor_a11y::record(&id, &name, _state, &description);
    #[cfg(feature = "gui-e2e")]
    super::conflict_editor_a11y::record_button_role(&id, role);
    let pointer_click = move |event: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
        cx.stop_propagation();
        if enabled {
            pointer(event, window, cx);
        }
    };
    let button_id = SharedString::from(format!("{id}-button"));
    let button = if enabled && role == ButtonRole::NeutralTinted {
        super::button_style::readable_adoption_button(button_id, content, background)
            .aria_label(SharedString::from(name.clone()))
            .on_click(pointer_click)
            .into_any_element()
    } else {
        KagiButton::styled(button_id, content, role, cx)
            .when(role == ButtonRole::NeutralTinted, |button| button.outline())
            .small()
            .h(super::theme::scaled_px(20.))
            .rounded(super::theme::scaled_px(5.))
            .text_size(super::theme::scaled_px(11.5))
            .tab_stop(false)
            .disabled(!enabled)
            .aria_label(SharedString::from(name.clone()))
            .on_click(pointer_click)
            .into_any_element()
    };
    let mut el = gpui::div()
        .id(SharedString::from(id.clone()))
        .role(Role::Button)
        .aria_label(SharedString::from(name))
        .aria_description(description)
        .when(hunk.is_some(), |el| el.key_context("KagiConflictResolver"))
        .h(super::theme::scaled_px(if hunk.is_some() {
            24.
        } else {
            20.
        }))
        .flex_shrink_0()
        .on_key_down(keyboard_nav::stop_activation_keys)
        .on_click(move |event, window, cx| {
            if enabled {
                activate(event, window, cx);
            }
        })
        .child(button);
    if enabled {
        el = attach_navigation(el, hunk, &id, owner);
    }
    #[cfg(feature = "gui-e2e")]
    {
        let probe_id = id.clone();
        el = el.on_key_down(move |event, _, cx| {
            if event.keystroke.key == "f19" {
                super::conflict_editor_a11y::focus(&probe_id);
                cx.stop_propagation();
            }
        });
    }
    el.relative().child(super::e2e::measure_inside(id))
}

pub(crate) fn attach_navigation(
    mut el: gpui::Stateful<gpui::Div>,
    hunk: Option<usize>,
    id: &str,
    owner: &Rc<RefCell<ResolverKeyboard>>,
) -> gpui::Stateful<gpui::Div> {
    let Some(hunk) = hunk else {
        return keyboard_nav::focusable(el);
    };
    let owner_ref = owner.borrow();
    let list = owner_ref.list.as_ref().expect("prepared resolver RowList");
    // A page change lays out new virtual rows after prepare chose its Tab stop.
    // Reconcile against this processor's range, not the previous page's bounds.
    let list = list.as_drawn(&owner_ref.drawn);
    if let Some((_, handle, _)) = owner_ref.controls[hunk]
        .iter()
        .find(|(key, _, _)| key == id)
    {
        let header_handle = list.focus_handle(hunk).clone();
        let header_row = owner_ref.rows[hunk].1;
        let header_owner = owner.clone();
        el = list.navigation(
            hunk,
            keyboard_nav::with_ring(el.track_focus(&handle.clone().tab_stop(false))).on_action(
                move |_: &ReturnToHeader, window, cx| {
                    if let Some(list) = &header_owner.borrow().list {
                        list.reveal_entry(header_row);
                    }
                    header_handle.focus(window, cx);
                },
            ),
        );
    } else {
        let preferred = owner_ref
            .rows
            .get(owner_ref.tab_hunk)
            .filter(|(_, row)| owner_ref.drawn.contains(row));
        let is_stop = if preferred.is_some() {
            hunk == owner_ref.tab_hunk
        } else {
            list.is_tab_stop(hunk)
        };
        let handle = list
            .focus_handle(hunk)
            .clone()
            .tab_index(0)
            .tab_stop(is_stop);
        el = list.navigation(hunk, keyboard_nav::with_ring(el.track_focus(&handle)));
    }
    let inline = owner_ref.controls[hunk].iter().any(|(key, _, _)| key == id);
    el = list.list(el).key_context(if inline {
        "KagiRowList KagiConflictResolver KagiConflictResolverControl"
    } else {
        "KagiRowList KagiConflictResolver"
    });
    drop(owner_ref);
    let prev_owner = owner.clone();
    let next_owner = owner.clone();
    let prev_id = id.to_owned();
    let next_id = id.to_owned();
    let tab_prev_owner = owner.clone();
    let tab_next_owner = owner.clone();
    let tab_prev_id = id.to_owned();
    let tab_next_id = id.to_owned();
    el = el
        .on_action(move |_: &PreviousTabControl, window, cx| {
            let owner = tab_prev_owner.borrow();
            if owner.controls[hunk]
                .iter()
                .any(|(key, _, _)| key == &tab_prev_id)
            {
                owner.move_control(hunk, &tab_prev_id, -1, window, cx);
            } else {
                window.focus_prev(cx);
            }
        })
        .on_action(move |_: &NextTabControl, window, cx| {
            let owner = tab_next_owner.borrow();
            if owner.controls[hunk]
                .last()
                .is_some_and(|(key, _, _)| key == &tab_next_id)
            {
                window.focus_next(cx);
            } else {
                owner.move_control(hunk, &tab_next_id, 1, window, cx);
            }
        });
    el.on_action(move |_: &PreviousControl, window, cx| {
        prev_owner
            .borrow()
            .move_control(hunk, &prev_id, -1, window, cx)
    })
    .on_action(move |_: &NextControl, window, cx| {
        next_owner
            .borrow()
            .move_control(hunk, &next_id, 1, window, cx)
    })
}

/// Paint the neighbouring conflict line without changing panes or the roving owner.
#[allow(clippy::too_many_arguments)]
pub(crate) fn line_navigation(
    el: gpui::Stateful<gpui::Div>,
    hunk: usize,
    side: SelectionSide,
    line: usize,
    taken: bool,
    enabled: bool,
    owner: &Rc<RefCell<ResolverKeyboard>>,
    entity: &gpui::Entity<ConflictView>,
) -> gpui::Stateful<gpui::Div> {
    let side_key = match side {
        SelectionSide::Current => "current",
        SelectionSide::Incoming => "incoming",
    };
    let id = format!("resolver-hunk-{hunk}-{side_key}-line-{line}");
    let mut el = el.on_key_down(keyboard_nav::stop_activation_keys);
    if enabled {
        el = attach_navigation(el, Some(hunk), &id, owner);
        let owner = owner.clone();
        let entity = entity.clone();
        el = el.on_key_down(move |event, window, cx| {
            let key = &event.keystroke;
            if !key.modifiers.shift
                || key.modifiers.control
                || key.modifiers.alt
                || key.modifiers.platform
                || !matches!(key.key.as_str(), "up" | "down")
            {
                return;
            }
            cx.stop_propagation();
            let neighbour = owner
                .borrow()
                .neighbour_line(hunk, side, line, key.key == "down");
            if let Some((next_hunk, next_line)) = neighbour {
                entity.update(cx, |view, cx| {
                    if let Some(path) = view.editing.clone() {
                        view.conflict_editor_set_hunk_line(
                            &path, next_hunk, side, next_line, taken,
                        );
                        cx.notify();
                    }
                });
                owner
                    .borrow()
                    .focus_line(next_hunk, side, next_line, window, cx);
            }
        });
    }
    #[cfg(feature = "gui-e2e")]
    {
        el = el.on_key_down(move |event, _, cx| {
            if event.keystroke.key == "f19" {
                super::conflict_editor_a11y::focus(&id);
                cx.stop_propagation();
            }
        });
    }
    el
}
