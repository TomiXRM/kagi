//! One hunk RowList, with horizontal adoption controls sharing its navigation.
use super::{
    conflict_view::ConflictView,
    keyboard_nav::{self, RowFocus, RowKeys, RowList, RowScroll},
};
use gpui::{
    actions, prelude::*, App, Context, FocusHandle, KeyBinding, Role, SharedString, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    Sizable,
};
use kagi_git::resolution::{HunkModel, Region};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

actions!(conflict_resolver, [PreviousControl, NextControl]);
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", PreviousControl, Some("KagiConflictResolver")),
        KeyBinding::new("right", NextControl, Some("KagiConflictResolver")),
    ]);
}
#[derive(Default)]
pub struct ResolverKeyboard {
    path: PathBuf,
    rows: RowKeys,
    focus: RowFocus,
    list: Option<RowList>,
    controls: Vec<Vec<(String, FocusHandle, usize)>>,
    pub drawn: std::ops::Range<usize>,
}
impl ResolverKeyboard {
    pub(crate) fn prepare(
        &mut self,
        path: Option<(&Path, &HunkModel)>,
        scroll: &gpui::UniformListScrollHandle,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some((path, model)) = path else {
            self.release(window, cx);
            return;
        };
        if self.path != path {
            self.release(window, cx);
            self.path = path.into();
            let mut rows = Vec::new();
            let mut row = 0;
            for region in &model.regions {
                match region {
                    Region::Passthrough(lines) => row += lines.len(),
                    Region::Hunk(hunk) => {
                        let n = rows.len();
                        rows.push((format!("resolver-hunk-{n}-current"), row));
                        let mut controls = vec![
                            (
                                format!("resolver-hunk-{n}-incoming"),
                                cx.focus_handle(),
                                row,
                            ),
                            (
                                format!("resolver-hunk-{n}-current-first"),
                                cx.focus_handle(),
                                row,
                            ),
                            (
                                format!("resolver-hunk-{n}-incoming-first"),
                                cx.focus_handle(),
                                row,
                            ),
                        ];
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
                        row += 1 + hunk.current.len().max(hunk.incoming.len());
                    }
                }
            }
            self.rows = rows.into();
        }
        self.list = Some(self.focus.rows(
            self.rows.clone(),
            &RowScroll::Uniform(scroll.clone(), self.drawn.clone()),
            None,
            window,
            cx,
        ));
    }
    fn release(&mut self, window: &mut Window, cx: &mut App) {
        if self
            .controls
            .iter()
            .flatten()
            .any(|(_, handle, _)| handle.is_focused(window))
            || self.focus.focused(window).is_some()
        {
            window.blur();
        }
        self.focus.release(None, window, cx);
        self.path.clear();
        self.controls.clear();
        self.rows = Rc::default();
        self.list = None;
        self.drawn = 0..0;
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
            self.list.as_ref().unwrap().focus_row(hunk, window, cx);
        } else {
            let (_, handle, row) = &controls[to - 1];
            if let Some(list) = &self.list {
                list.reveal_entry(*row);
            }
            handle.focus(window, cx);
        }
    }
}
/// The pinned Button owns pointer feedback; its enclosing roving owner handles
/// keyboard clicks. Both call the very same listener, never duplicate mutation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn adoption(
    id: String,
    name: String,
    state: &'static str,
    content: impl IntoElement,
    hunk: Option<usize>,
    owner: &Rc<RefCell<ResolverKeyboard>>,
    activate: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    _cx: &mut Context<ConflictView>,
) -> gpui::Stateful<gpui::Div> {
    let activate = Rc::new(activate);
    let pointer = activate.clone();
    #[cfg(feature = "gui-e2e")]
    super::conflict_editor_a11y::record(&id, &name, state);
    let description = match state {
        "all" => super::i18n::Msg::EditorSelectionAll,
        "partial" => super::i18n::Msg::EditorSelectionPartial,
        _ => super::i18n::Msg::EditorSelectionNone,
    }
    .t();
    let button = super::button_style::named_button(
        Button::new(SharedString::from(format!("{id}-button")))
            .ghost()
            .xsmall()
            .compact()
            .tab_stop(false)
            .p(gpui::px(0.))
            .size_full()
            .child(content)
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                pointer(event, window, cx);
            }),
        SharedString::from(name.clone()),
        Some(state == "all"),
    );
    let mut el = gpui::div()
        .id(SharedString::from(id.clone()))
        .role(Role::Button)
        .aria_label(SharedString::from(name))
        .aria_description(description)
        .aria_toggled(if state == "all" {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .key_context("KagiConflictResolver")
        .on_key_down(keyboard_nav::stop_activation_keys)
        .on_click(move |event, window, cx| activate(event, window, cx))
        .child(button);
    el = attach_navigation(el, hunk, &id, owner);
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
    if let Some((_, handle, _)) = owner_ref.controls[hunk]
        .iter()
        .find(|(key, _, _)| key == id)
    {
        el = list.navigation(
            hunk,
            keyboard_nav::with_ring(el.track_focus(&handle.clone().tab_stop(false))),
        );
    } else {
        el = list.row(hunk, el);
    }
    el = list
        .list(el)
        .key_context("KagiRowList KagiConflictResolver");
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
