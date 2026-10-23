//! Whole-file hunk RowList, with horizontal adoption controls sharing navigation.
use super::{
    conflict_view::ConflictView,
    keyboard_nav::{self, RowFocus, RowKeys, RowList, RowScroll},
};
use gpui::{
    actions, prelude::*, App, Context, FocusHandle, KeyBinding, Role, SharedString, Window,
};
use gpui_component::Disableable as _;
use gpui_component::{
    button::{Button, ButtonVariants},
    Sizable,
};
use kagi_git::resolution::{HunkModel, Region, SelectionSide, TriState};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

actions!(
    conflict_resolver,
    [PreviousControl, NextControl, ReturnToHeader]
);
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", PreviousControl, Some("KagiConflictResolver")),
        KeyBinding::new("right", NextControl, Some("KagiConflictResolver")),
        KeyBinding::new("up", ReturnToHeader, Some("KagiConflictResolverControl")),
    ]);
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum RegionShape {
    Context(usize),
    Hunk(usize, usize),
}
impl RegionShape {
    fn of(region: &Region) -> Self {
        match region {
            Region::Passthrough(lines) => Self::Context(lines.len()),
            Region::Hunk(hunk) => Self::Hunk(hunk.current.len(), hunk.incoming.len()),
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
                            scroll.scroll_to_item(row.saturating_sub(3), gpui::ScrollStrategy::Top);
                        }
                        rows.push((format!("conflict-choice-{n}"), row));
                        let mut controls = Vec::new();
                        for (side, lines) in
                            [("current", &hunk.current), ("incoming", &hunk.incoming)]
                        {
                            for line in 0..lines.len() {
                                controls.push((
                                    format!("resolver-hunk-{n}-{side}-line-{line}"),
                                    cx.focus_handle(),
                                    row + line,
                                ));
                            }
                        }
                        self.controls.push(controls);
                        row += hunk.current.len().max(hunk.incoming.len()).max(1);
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
                    list.focus_row(self.tab_hunk, window, cx);
                }
            }
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
        focused
    }
    pub(crate) fn focus_hunk(&self, hunk: usize, window: &mut Window, cx: &mut App) {
        if let Some(list) = &self.list {
            list.focus_row(hunk, window, cx);
        }
    }
    pub(crate) fn hunk_focused(&self, hunk: usize, window: &Window) -> bool {
        self.list
            .as_ref()
            .is_some_and(|list| hunk < list.len() && list.focus_handle(hunk).is_focused(window))
    }
    pub(crate) fn side_name(&self, side: SelectionSide) -> &str {
        &self.side_names[match side {
            SelectionSide::Current => 0,
            SelectionSide::Incoming => 1,
        }]
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
pub(crate) enum ControlState {
    Adoption(TriState),
    Choice(String),
}
impl ControlState {
    fn parts(self) -> (&'static str, SharedString, bool) {
        use super::i18n::Msg;
        match self {
            Self::Adoption(state) => {
                let (key, message) = match state {
                    TriState::None => ("none", Msg::EditorSelectionNone),
                    TriState::Partial => ("partial", Msg::EditorSelectionPartial),
                    TriState::All => ("all", Msg::EditorSelectionAll),
                };
                (key, message.t().into(), state == TriState::All)
            }
            Self::Choice(label) => {
                use super::i18n::Msg;
                let state = if label == Msg::ConflictChoiceLeft.t() {
                    "left"
                } else if label == Msg::ConflictChoiceRight.t() {
                    "right"
                } else if label == "?" {
                    "none"
                } else {
                    "both"
                };
                (state, label.into(), state != "none")
            }
        }
    }
}
/// The pinned Button owns pointer feedback; its enclosing roving owner handles
/// keyboard clicks. Both call the very same listener, never duplicate mutation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn adoption(
    id: String,
    name: String,
    state: ControlState,
    content: impl IntoElement,
    hunk: Option<usize>,
    enabled: bool,
    owner: &Rc<RefCell<ResolverKeyboard>>,
    activate: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    _cx: &mut Context<ConflictView>,
) -> gpui::Stateful<gpui::Div> {
    let activate = Rc::new(activate);
    let pointer = activate.clone();
    let (_state, description, toggled) = state.parts();
    #[cfg(feature = "gui-e2e")]
    super::conflict_editor_a11y::record(&id, &name, _state, &description);
    let button = super::button_style::named_button(
        Button::new(SharedString::from(format!("{id}-button")))
            .ghost()
            .xsmall()
            .compact()
            .tab_stop(false)
            .disabled(!enabled)
            .p(gpui::px(0.))
            .size_full()
            .child(content)
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                if enabled {
                    pointer(event, window, cx);
                }
            }),
        SharedString::from(name.clone()),
        Some(toggled),
    );
    let mut el = gpui::div()
        .id(SharedString::from(id.clone()))
        .role(Role::Button)
        .aria_label(SharedString::from(name))
        .aria_description(description)
        .aria_toggled(if toggled {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .key_context("KagiConflictResolver")
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

fn attach_navigation(
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
