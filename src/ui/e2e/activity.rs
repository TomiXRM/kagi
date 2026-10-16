//! Attributes installed by the Activity renderer, not native VoiceOver evidence.
use gpui::{FocusHandle, Role};
use std::cell::RefCell;

#[derive(Clone, Debug)]
pub struct Node {
    pub role: Option<Role>,
    pub name: String,
    pub selected: bool,
    pub tab_stop: bool,
    pub focus: Option<FocusHandle>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Stroke {
    pub solid_segments: usize,
    pub dash_segments: usize,
    pub gap_segments: usize,
    pub knockout_segments: usize,
    pub single_point: bool,
}
thread_local! {
    static NODES: RefCell<std::collections::HashMap<String, Node>> = RefCell::default();
    static STROKES: RefCell<std::collections::HashMap<String, Stroke>> = RefCell::default();
    static READOUT: RefCell<String> = RefCell::default();
    static POINTS: RefCell<Vec<(usize, bool, gpui::Point<gpui::Pixels>)>> = RefCell::default();
    static PLOT_WIDTH: RefCell<Option<gpui::Pixels>> = const { RefCell::new(None) };
}
pub(crate) fn begin() {
    NODES.with(|nodes| nodes.borrow_mut().clear());
    STROKES.with(|strokes| strokes.borrow_mut().clear());
    POINTS.with(|points| points.borrow_mut().clear());
}
pub(crate) fn record(id: &str, node: Node) {
    NODES.with(|nodes| nodes.borrow_mut().insert(id.to_string(), node));
}
pub fn node(id: &str) -> Option<Node> {
    NODES.with(|nodes| nodes.borrow().get(id).cloned())
}
pub fn nodes(prefix: &str) -> Vec<(String, Node)> {
    NODES.with(|nodes| {
        nodes
            .borrow()
            .iter()
            .filter(|(id, _)| id.starts_with(prefix))
            .map(|(id, node)| (id.clone(), node.clone()))
            .collect()
    })
}
pub(crate) fn stroke(id: &str, stroke: Stroke) {
    STROKES.with(|strokes| strokes.borrow_mut().insert(id.to_string(), stroke));
}
pub fn painted_stroke(id: &str) -> Option<Stroke> {
    STROKES.with(|strokes| strokes.borrow().get(id).copied())
}
pub(crate) fn record_readout(text: &str) {
    READOUT.with(|readout| *readout.borrow_mut() = text.to_string());
}
pub fn readout() -> String {
    READOUT.with(|readout| readout.borrow().clone())
}
pub(crate) fn record_point(index: usize, merges: bool, point: gpui::Point<gpui::Pixels>) {
    POINTS.with(|points| points.borrow_mut().push((index, merges, point)));
}
pub fn painted_points() -> Vec<(usize, bool, gpui::Point<gpui::Pixels>)> {
    POINTS.with(|points| points.borrow().clone())
}
/// Layout-only probe: both production painter and overlay receive this width.
pub struct PlotWidth;
impl PlotWidth {
    pub fn set(width: gpui::Pixels) -> Self {
        PLOT_WIDTH.with(|value| *value.borrow_mut() = Some(width));
        Self
    }
}
impl Drop for PlotWidth {
    fn drop(&mut self) {
        PLOT_WIDTH.with(|value| *value.borrow_mut() = None);
    }
}
pub(crate) fn plot_width() -> Option<gpui::Pixels> {
    PLOT_WIDTH.with(|value| *value.borrow())
}
