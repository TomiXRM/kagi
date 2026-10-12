//! Native render-boundary evidence for complete commit text (#1306/#1307).
use gpui::SharedString;
use std::cell::RefCell;

#[derive(Clone, Debug)]
pub struct Message {
    pub source: SharedString,
    pub html: SharedString,
    pub coauthors: usize,
    pub trailers: usize,
}
thread_local! {
    static MESSAGE: RefCell<Option<Message>> = const { RefCell::new(None) };
    static TOOLTIP: RefCell<Option<SharedString>> = const { RefCell::new(None) };
}
pub(crate) fn record_message(message: Message) {
    MESSAGE.with(|slot| *slot.borrow_mut() = Some(message));
}
pub fn message() -> Message {
    MESSAGE.with(|slot| slot.borrow().clone().expect("rendered commit message"))
}
pub(crate) fn record_tooltip(subject: SharedString) {
    TOOLTIP.with(|slot| *slot.borrow_mut() = Some(subject));
}
pub fn tooltip() -> Option<SharedString> {
    TOOLTIP.with(|slot| slot.borrow().clone())
}
pub fn clear() {
    MESSAGE.with(|slot| *slot.borrow_mut() = None);
    TOOLTIP.with(|slot| *slot.borrow_mut() = None);
}
