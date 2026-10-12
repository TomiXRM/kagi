//! Renderer-fed clone label semantics, with no predicted accessibility values.
use gpui::Role;

#[derive(Clone, Debug)]
pub struct Presentation {
    pub role: Option<Role>,
    pub name: Option<String>,
    pub visible: String,
    pub seconds: u64,
}

thread_local! {
    static PRESENTATION: std::cell::RefCell<Option<Presentation>> = const {
        std::cell::RefCell::new(None)
    };
}

pub(crate) fn record(role: Option<Role>, name: Option<&str>, visible: &str, seconds: u64) {
    PRESENTATION.with(|slot| {
        *slot.borrow_mut() = Some(Presentation {
            role,
            name: name.map(str::to_owned),
            visible: visible.to_owned(),
            seconds,
        });
    });
}

pub fn take() -> Option<Presentation> {
    PRESENTATION.with(|slot| slot.borrow_mut().take())
}
