//! Renderer attribute and geometry probes, opt-in for native Tier A.
use gpui::{prelude::*, AnyElement, Bounds, Pixels, Role, WindowId};
use std::cell::RefCell;
use std::collections::HashMap;
#[derive(Clone, Debug)]
pub struct Node {
    pub role: Role,
    pub name: String,
    pub text: String,
    pub selected: Option<bool>,
    pub expanded: Option<bool>,
    pub disabled: bool,
}
thread_local! {
    static NODES: RefCell<HashMap<String, Node>> = RefCell::new(HashMap::new());
    static BOUNDS: RefCell<HashMap<(WindowId, String), Bounds<Pixels>>> = RefCell::new(HashMap::new());
}
pub fn clear() {
    NODES.with(|m| m.borrow_mut().clear());
    BOUNDS.with(|m| m.borrow_mut().clear());
}
pub fn node(id: &str) -> Option<Node> {
    NODES.with(|m| m.borrow().get(id).cloned())
}
pub fn count(prefix: &str) -> usize {
    NODES.with(|m| {
        m.borrow()
            .keys()
            .filter(|id| id.starts_with(prefix))
            .count()
    })
}
pub fn choices() -> Vec<(String, Node)> {
    NODES.with(|m| {
        m.borrow()
            .iter()
            .filter(|(_, node)| node.role == Role::Tab)
            .map(|(id, node)| (id.clone(), node.clone()))
            .collect()
    })
}
pub fn bounds(window: WindowId, id: &str) -> Option<Bounds<Pixels>> {
    BOUNDS.with(|m| m.borrow().get(&(window, id.into())).copied())
}
pub fn record(id: &str, node: Node) {
    NODES.with(|m| m.borrow_mut().insert(id.into(), node));
}
pub(crate) fn measure(id: impl Into<String>) -> AnyElement {
    let id = id.into();
    gpui::canvas(
        move |bounds, window, _| {
            BOUNDS.with(|m| {
                m.borrow_mut()
                    .insert((window.window_handle().window_id(), id.clone()), bounds)
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}
