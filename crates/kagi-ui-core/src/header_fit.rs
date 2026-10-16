//! Pane headers whose buttons always stay reachable (#809).
//!
//! A header row is `[buttons…] [title] [buttons…]`. The title truncates first
//! (`flex_1` + `min_w(0)`); when even an empty-ish title leaves too little
//! room for the labelled buttons, the buttons fall back to their icons, with
//! the label as the tooltip. There is no overflow menu.
//!
//! "Too little room" is measured, not guessed: [`HeaderFit::probes`] adds
//! the row's own width probe and an off-screen copy of the labelled buttons
//! (same `Button` style, layout only) whose natural width is what the labels
//! need in the current language and zoom. The copy never prepaints or paints,
//! so it contributes no accessibility nodes, focus or hit targets. Both
//! measurements land at prepaint; a changed decision asks for one more frame.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{canvas, div, px, rems, AnyElement, Bounds, Pixels, SharedString};
use gpui_component::button::Button;
use gpui_component::{Icon, Sizable as _};

use crate::tooltip::HoverTooltip as _;

/// The title keeps at least this much room before the buttons give up their
/// labels, so the file name never disappears entirely.
const MIN_TITLE_REM: f32 = 8.;

/// Where the labelled copy is laid out: far left of the row, outside the
/// window and therefore outside every hit test.
const OFFSCREEN: f32 = -100_000.;

#[derive(Default)]
struct Measured {
    row: Cell<Option<Bounds<Pixels>>>,
    natural: Cell<Option<Pixels>>,
    controls: RefCell<HashMap<&'static str, Bounds<Pixels>>>,
}

/// One header's measurements; clone it into the pane that renders the row.
#[derive(Clone, Default)]
pub struct HeaderFit(Rc<Measured>);

impl HeaderFit {
    /// Show icons instead of labels: the labelled buttons plus the title's
    /// minimum do not fit the row as last laid out.
    pub fn compact(&self) -> bool {
        match (self.0.row.get(), self.0.natural.get()) {
            (Some(row), Some(natural)) => row.size.width < natural,
            _ => false,
        }
    }

    /// The row's bounds as last laid out.
    pub fn row_bounds(&self) -> Option<Bounds<Pixels>> {
        self.0.row.get()
    }

    /// A control's bounds as last laid out (see [`Self::control`]).
    pub fn control_bounds(&self, name: &str) -> Option<Bounds<Pixels>> {
        self.0.controls.borrow().get(name).copied()
    }

    /// Record `control`'s bounds under `name` every frame. The control keeps
    /// its full width: a button never shrinks.
    pub fn control(&self, name: &'static str, control: impl IntoElement) -> AnyElement {
        self.recorded(name, div().flex_shrink_0(), control)
    }

    /// [`Self::control`] for a text item that, once the title is gone and
    /// the buttons are icons, is the last thing to give way (it truncates).
    pub fn text_control(&self, name: &'static str, text: impl IntoElement) -> AnyElement {
        self.recorded(name, div().min_w(px(0.)).overflow_hidden(), text)
    }

    fn recorded(
        &self,
        name: &'static str,
        wrapper: gpui::Div,
        control: impl IntoElement,
    ) -> AnyElement {
        let fit = self.clone();
        wrapper
            .relative()
            .child(control)
            .child(
                canvas(
                    move |bounds, _, _| {
                        fit.0.controls.borrow_mut().insert(name, bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .into_any_element()
    }

    /// Two more children for a `relative()` header row styled `px_3` +
    /// `gap_2`: the row's width probe and the off-screen labelled copy of its
    /// `labels` (buttons) and `texts` (plain `text_sm` items).
    pub fn probes(&self, labels: &[SharedString], texts: &[SharedString]) -> [AnyElement; 2] {
        let row = self.clone();
        let row_probe = canvas(
            move |bounds, window, _| {
                let before = row.compact();
                row.0.row.set(Some(bounds));
                if row.compact() != before {
                    window.request_animation_frame();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element();

        let copy = div()
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .px_3()
            .gap_2()
            .child(div().flex_shrink_0().w(rems(MIN_TITLE_REM)))
            .children(labels.iter().enumerate().map(|(ix, label)| {
                Button::new(("header-fit-copy", ix))
                    .label(label.clone())
                    .outline()
                    .small()
                    .tab_stop(false)
            }))
            .children(
                texts
                    .iter()
                    .map(|text| div().flex_shrink_0().text_sm().child(text.clone())),
            );
        // A wide absolute box so the copy takes its max-content width.
        let offscreen = div()
            .absolute()
            .top_0()
            .left(px(OFFSCREEN))
            .w(px(-OFFSCREEN / 2.))
            .flex()
            .flex_row()
            .child(NaturalWidthProbe {
                copy,
                fit: self.clone(),
            })
            .into_any_element();
        [row_probe, offscreen]
    }
}

/// Keep the exact Button layout without entering its interactive/AX lifecycle.
struct NaturalWidthProbe {
    copy: gpui::Div,
    fit: HeaderFit,
}

impl IntoElement for NaturalWidthProbe {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for NaturalWidthProbe {
    type RequestLayoutState = <gpui::Div as gpui::Element>::RequestLayoutState;
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        self.copy.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut gpui::Window,
        _: &mut gpui::App,
    ) {
        let before = self.fit.compact();
        self.fit.0.natural.set(Some(bounds.size.width));
        if self.fit.compact() != before {
            window.request_animation_frame();
        }
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        _: &mut gpui::Window,
        _: &mut gpui::App,
    ) {
    }
}

/// A header button: its label, or — when `compact` — only `icon`, with the
/// label as the tooltip.
pub fn header_button(
    id: &'static str,
    label: impl Into<SharedString>,
    icon: &'static str,
    compact: bool,
    configure: impl FnOnce(Button) -> Button,
) -> AnyElement {
    let label = label.into();
    // `outline`, not `ghost`: a ghost button paints no background of its own,
    // so it took the header bar's colour exactly and read as plain text.
    let button = NamedHeaderButton(Button::new(id).outline().small())
        .role(gpui::Role::Button)
        .aria_label(label.clone())
        .0;
    if compact {
        configure(button.icon(Icon::empty().path(icon)))
            .hover_tooltip((id, 0usize), label)
            .into_any_element()
    } else {
        configure(button.label(label)).into_any_element()
    }
}

// The pinned Button exposes its interactivity, but not GPUI's stateful AX
// setters. Adapt only those setters; the actual Button still owns rendering,
// focus, selected paint and click handling (including in icon-only mode).
struct NamedHeaderButton(Button);

impl gpui::InteractiveElement for NamedHeaderButton {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.0.interactivity()
    }
}

impl gpui::StatefulInteractiveElement for NamedHeaderButton {}
