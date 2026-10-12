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
thread_local! {
    static NODES: RefCell<std::collections::HashMap<String, Node>> = RefCell::default();
    static STROKES: RefCell<std::collections::HashMap<String, (bool, usize)>> = RefCell::default();
    static READOUT: RefCell<String> = RefCell::default();
}
pub(crate) fn begin() {
    NODES.with(|nodes| nodes.borrow_mut().clear());
    STROKES.with(|strokes| strokes.borrow_mut().clear());
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
pub(crate) fn stroke(id: &str, dashed: bool, segments: usize) {
    STROKES.with(|strokes| {
        strokes
            .borrow_mut()
            .insert(id.to_string(), (dashed, segments))
    });
}
pub fn painted_stroke(id: &str) -> Option<(bool, usize)> {
    STROKES.with(|strokes| strokes.borrow().get(id).copied())
}
pub(crate) fn record_readout(text: &str) {
    READOUT.with(|readout| *readout.borrow_mut() = text.to_string());
}
pub fn readout() -> String {
    READOUT.with(|readout| readout.borrow().clone())
}
