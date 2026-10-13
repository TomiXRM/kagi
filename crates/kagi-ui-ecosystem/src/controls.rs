//! Dense Analyze controls using the shared roving-tab and pinned Button contracts.
use super::*;
use gpui::{App, Div, FocusHandle, Role, Stateful};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    Disableable as _, Sizable as _,
};
use kagi_ui_core::keyboard_nav::{self, Activation, TabFocus, TabList};
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct Navigation {
    pub mode: TabFocus,
    pub period: TabFocus,
    pub view: TabFocus,
    pub coupling_view: TabFocus,
    pub format: TabFocus,
    pub buttons: RefCell<HashMap<String, FocusHandle>>,
    pub root: Option<FocusHandle>,
}
impl Navigation {
    pub fn handle(&self, id: &str, cx: &App) -> FocusHandle {
        if let Some(handle) = self.buttons.borrow().get(id) {
            return handle.clone();
        }
        let handle = cx.focus_handle();
        self.buttons.borrow_mut().insert(id.into(), handle.clone());
        handle
    }
    pub fn retire_hidden(&self, data: &EcosystemData, window: &mut Window, cx: &mut App) {
        if data.mode != EcosystemMode::Hotspots {
            self.view.sync(&[], cx);
        }
        if data.mode != EcosystemMode::Coupling {
            self.coupling_view.sync(&[], cx);
        }
        self.format.sync(
            if data.mode == EcosystemMode::Coupling {
                &[0, 1, 2]
            } else {
                &[0, 1]
            },
            cx,
        );
        for focus in [&self.view, &self.coupling_view, &self.format] {
            focus.release_closed(self.root.as_ref(), window, cx);
        }
        let hidden_focus = self
            .buttons
            .borrow()
            .iter()
            .any(|(id, handle)| handle.is_focused(window) && !row_visible(id, data));
        if hidden_focus {
            if let Some(root) = &self.root {
                root.focus(window, cx);
            }
        }
    }
}

// A period refresh can remove a focused row without changing the selected mode.
fn row_visible(id: &str, data: &EcosystemData) -> bool {
    let (rank, count) = if let Some(rank) = id.strip_prefix("eco-hot-row-") {
        let count = if data.mode == EcosystemMode::Hotspots && !data.map {
            data.ecosystem
                .as_ref()
                .map_or(0, |eco| eco.files.len().min(super::lists::MAX_ROWS))
        } else {
            0
        };
        (rank, count)
    } else if let Some(rank) = id.strip_prefix("eco-coup-row-") {
        let count =
            if data.mode == EcosystemMode::Coupling && data.coupling_view == CouplingView::List {
                data.couplings.len()
            } else {
                0
            };
        (rank, count)
    } else {
        return true;
    };
    !data.loading
        && data.error.is_none()
        && rank
            .parse::<usize>()
            .is_ok_and(|rank| rank > 0 && rank <= count)
}

pub(crate) fn tabs<T: Copy + PartialEq + 'static>(
    id: &'static str,
    focus: &TabFocus,
    choices: &[(T, &'static str, String)],
    active: T,
    pointer_focus: Option<FocusHandle>,
    setter: fn(&mut EcosystemView, T, &mut Context<EcosystemView>),
    cx: &mut Context<EcosystemView>,
) -> gpui::AnyElement {
    let values: Vec<_> = choices.iter().map(|(v, _, _)| *v).collect();
    let entity = cx.entity().downgrade();
    let list = TabList::new(
        focus,
        keyboard_nav::slot_keys(choices.len()),
        (0..choices.len()).collect(),
        choices.iter().position(|(v, _, _)| *v == active),
        Activation::Automatic,
        pointer_focus,
        move |slot, _, _, cx| {
            let _ = entity.update(cx, |view, cx| setter(view, values[slot], cx));
        },
        cx,
    );
    let name = match id {
        "eco-modes" => Msg::EcoMode,
        "eco-periods" => Msg::EcoPeriod,
        "eco-formats" => Msg::EcoExportFormat,
        _ => Msg::EcoView,
    }
    .t();
    #[cfg(feature = "gui-e2e")]
    super::e2e::record(
        id,
        super::e2e::Node {
            role: Role::TabList,
            name: name.into(),
            text: name.into(),
            selected: None,
            expanded: None,
            disabled: false,
        },
    );
    let mut row = list
        .list(div().id(id).flex().items_center().gap_1())
        .aria_label(name);
    for (slot, (value, label, cell_id)) in choices.iter().enumerate() {
        #[cfg(feature = "gui-e2e")]
        super::e2e::record(
            cell_id,
            super::e2e::Node {
                role: Role::Tab,
                name: (*label).into(),
                text: (*label).into(),
                selected: Some(*value == active),
                expanded: None,
                disabled: false,
            },
        );
        let cell = tab_paint(cell_id, label, *value == active);
        let cell = probe(cell_id, list.cell(slot, label, cell));
        row = row.child(cell);
    }
    row.into_any_element()
}

fn tab_paint(id: &str, label: &'static str, selected: bool) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.to_string()))
        .relative()
        .px(keyboard_nav::inset(8.))
        .py(keyboard_nav::inset(5.))
        .min_h(theme::scaled_px(24.))
        .rounded(theme::scaled_px(4.))
        .bg(rgb(if selected {
            theme().ui_accent()
        } else {
            theme().surface
        }))
        .text_color(rgb(if selected {
            theme().on_accent()
        } else {
            theme().text_sub
        }))
        .text_size(theme::scaled_px(12.))
        .cursor_default()
        .child(label)
}

pub(crate) fn button(
    id: &'static str,
    caption: &str,
    name: &str,
    enabled: bool,
    focus: FocusHandle,
) -> Stateful<Div> {
    #[cfg(feature = "gui-e2e")]
    super::e2e::record(
        id,
        super::e2e::Node {
            role: Role::Button,
            name: name.into(),
            text: caption.into(),
            selected: None,
            expanded: None,
            disabled: !enabled,
        },
    );
    // The wrapper owns one named keyboard/AX target. Pinned Button is paint only,
    // avoiding a second Tab stop while allowing glyph captions to have full names.
    let paint = Button::new(SharedString::from(format!("{id}-paint")))
        .small()
        .ghost()
        .label(caption.to_string())
        .tooltip(name.to_string())
        .disabled(!enabled)
        .tab_stop(false);
    probe(
        id,
        keyboard_nav::with_ring(
            div()
                .id(id)
                .relative()
                .rounded(theme::scaled_px(6.))
                .role(Role::Button)
                .aria_label(SharedString::from(name.to_string()))
                .track_focus(&focus.tab_index(0).tab_stop(true))
                .a11y_synthetic_children(move |builder| {
                    if !enabled {
                        builder.parent_node().set_disabled();
                    }
                })
                .on_key_down(keyboard_nav::stop_activation_keys)
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-visual")))
                        .role(Role::Group)
                        .a11y_synthetic_children(|builder| builder.parent_node().set_hidden())
                        .child(paint),
                ),
        ),
    )
}

pub(crate) fn probe(id: &str, element: Stateful<Div>) -> Stateful<Div> {
    #[cfg(feature = "gui-e2e")]
    {
        element.child(super::e2e::measure(id))
    }
    #[cfg(not(feature = "gui-e2e"))]
    {
        let _ = id;
        element
    }
}
