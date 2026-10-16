//! Renderer-fed file-tree geometry; compiled only for native GUI E2E.
use gpui::{canvas, prelude::*, Bounds, Pixels};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[derive(Clone, Copy, Debug)]
pub struct EditorTreeCellProbe {
    pub bounds: Bounds<Pixels>,
    pub mask: Bounds<Pixels>,
    pub font_size: Pixels,
    pub font_weight: gpui::FontWeight,
    pub line_height: Pixels,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EditorTreeRowProbe {
    pub row: Option<EditorTreeCellProbe>,
    pub badge: Option<EditorTreeCellProbe>,
    pub name: Option<EditorTreeCellProbe>,
}

#[derive(Default)]
pub(crate) struct TreeProbes(pub Rc<RefCell<HashMap<usize, EditorTreeRowProbe>>>);

#[derive(Clone, Copy)]
pub(crate) enum Part {
    Row,
    Badge,
    Name,
}

pub(crate) fn measure(probes: &TreeProbes, index: usize, part: Part) -> impl IntoElement {
    let probes = probes.0.clone();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let style = window.text_style();
            let cell = Some(EditorTreeCellProbe {
                bounds,
                mask: window.content_mask().bounds,
                font_size: style.font_size.to_pixels(window.rem_size()),
                font_weight: style.font_weight,
                line_height: style.line_height_in_pixels(window.rem_size()),
            });
            let mut probes = probes.borrow_mut();
            let row = probes.entry(index).or_default();
            match part {
                Part::Row => row.row = cell,
                Part::Badge => row.badge = cell,
                Part::Name => row.name = cell,
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

impl crate::EditorWorkspaceView {
    /// Uses the base index in `tree`, not the visible position or file index.
    /// Absent rows were not painted in the current tree frame.
    pub fn tree_row_probe_for_e2e(&self, index: usize) -> Option<EditorTreeRowProbe> {
        self.tree_probes.0.borrow().get(&index).copied()
    }
}
