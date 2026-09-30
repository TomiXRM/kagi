//! Pane headers whose buttons always stay reachable (#809).
//!
//! A header row is `[buttons…] [title] [buttons…]`. The title truncates first
//! (`flex_1` + `min_w(0)`); when even an empty-ish title leaves too little
//! room for the labelled buttons, the buttons fall back to their icons, with
//! the label as the tooltip. There is no overflow menu.
//!
//! "Too little room" is measured, not guessed: [`HeaderFit::probes`] adds
//! the row's own width probe and an off-screen copy of the labelled buttons
//! (same `Button` style, no handlers, outside every content mask) whose
//! natural width is what the labels need in the current language and zoom.
//! Both land at prepaint; a change that flips the decision asks for one more
//! frame, so the header settles one frame after a resize.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{canvas, div, px, rems, AnyElement, Bounds, Pixels, SharedString};
use gpui_component::button::Button;
use gpui_component::{Icon, Sizable as _};

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

        let natural = self.clone();
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
            }))
            .children(
                texts
                    .iter()
                    .map(|text| div().flex_shrink_0().text_sm().child(text.clone())),
            )
            .child(
                canvas(
                    move |bounds, window, _| {
                        let before = natural.compact();
                        natural.0.natural.set(Some(bounds.size.width));
                        if natural.compact() != before {
                            window.request_animation_frame();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        // A wide absolute box so the copy takes its max-content width.
        let offscreen = div()
            .absolute()
            .top_0()
            .left(px(OFFSCREEN))
            .w(px(-OFFSCREEN / 2.))
            .flex()
            .flex_row()
            .child(copy)
            .into_any_element();
        [row_probe, offscreen]
    }
}

/// A header button: its label, or — when `compact` — only `icon`, with the
/// label as the tooltip.
pub fn header_button(
    id: &'static str,
    label: impl Into<SharedString>,
    icon: &'static str,
    compact: bool,
) -> Button {
    let label = label.into();
    // `outline`, not `ghost`: a ghost button paints no background of its own,
    // so it took the header bar's colour exactly and read as plain text.
    let button = Button::new(id).outline().small();
    if compact {
        button.icon(Icon::empty().path(icon)).tooltip(label)
    } else {
        button.label(label)
    }
}
