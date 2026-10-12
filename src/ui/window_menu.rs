//! Main-window native metadata and the shared stateful menu presentation.
use super::{
    commands,
    i18n::{self, Lang, Msg},
    KagiApp,
};
use gpui::{AnyWindowHandle, App, Context, Global, Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewMenuState {
    pub sidebar_visible: bool,
    pub fullscreen: bool,
}
impl Default for ViewMenuState {
    fn default() -> Self {
        Self {
            sidebar_visible: true,
            fullscreen: false,
        }
    }
}
#[derive(Default)]
pub(crate) struct WindowMenus {
    main_windows: Vec<AnyWindowHandle>,
    owner: Option<AnyWindowHandle>,
    state: ViewMenuState,
    language: Option<Lang>,
}
impl Global for WindowMenus {}

pub(crate) fn register_main_window(window: &Window, cx: &mut App) {
    if !cx.has_global::<WindowMenus>() {
        cx.set_global(WindowMenus::default());
        cx.on_window_closed(|cx, id| {
            let menus = cx.global_mut::<WindowMenus>();
            menus.main_windows.retain(|window| window.window_id() != id);
            if menus.owner.is_some_and(|window| window.window_id() == id) {
                menus.owner = None;
            }
        })
        .detach();
    }
    cx.global_mut::<WindowMenus>()
        .main_windows
        .push(window.window_handle());
}
pub(crate) fn current_state(cx: &App) -> ViewMenuState {
    cx.try_global::<WindowMenus>()
        .map(|menus| menus.state)
        .unwrap_or_default()
}
/// Used by both native menus and the in-app dropdown; IDs and bindings do not vary.
pub fn state_label(id: &str, state: ViewMenuState) -> Option<&'static str> {
    let msg = match id {
        "view.toggleSidebar" if state.sidebar_visible => Msg::MenuHideSidebar,
        "view.toggleSidebar" => Msg::MenuShowSidebar,
        "view.fullScreen" if state.fullscreen => Msg::MenuExitFullScreen,
        "view.fullScreen" => Msg::MenuEnterFullScreen,
        "window.previousTab" => Msg::MenuPreviousTab,
        "window.nextTab" => Msg::MenuNextTab,
        "window.bringAllToFront" => Msg::MenuBringAllToFront,
        "app.about" => Msg::AboutTitle,
        _ => return None,
    };
    Some(msg.t())
}
impl KagiApp {
    pub(super) fn sync_window_presentation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title =
            if self.home_in_front() || self.repo_path.is_none() && self.remote_view.is_none() {
                gpui::SharedString::from("Kagi")
            } else {
                self.view().window_title.clone()
            };
        // A just-opened tab may not have its first accepted read yet. Publish
        // its known repository identity immediately, never a blank/stale title.
        if title.is_empty() {
            let name = self
                .tabs
                .get(self.active_tab)
                .map(|tab| tab.name.as_str())
                .unwrap_or("Kagi");
            if self.native_window_title.as_ref() != name {
                window.set_window_title(name);
                self.native_window_title = name.to_owned().into();
            }
        } else if self.native_window_title != title {
            window.set_window_title(title.as_ref());
            self.native_window_title = title;
        }
        // Inactive renders retain the last owner, even while the app is inactive.
        // Only initial publication may occur without an active main window.
        if !self.window_active_for_render(window)
            && (cx.active_window().is_some() || cx.global::<WindowMenus>().owner.is_some())
        {
            return;
        }
        self.publish_window_menu_state(window, cx);
    }
    pub(super) fn publish_window_menu_state(&self, window: &Window, cx: &mut Context<Self>) {
        let state = ViewMenuState {
            sidebar_visible: self.sidebar.visible,
            fullscreen: window.is_fullscreen(),
        };
        let handle = window.window_handle();
        let language = i18n::lang();
        let menus = cx.global_mut::<WindowMenus>();
        if menus.owner == Some(handle) && menus.state == state && menus.language == Some(language) {
            return;
        }
        menus.owner = Some(handle);
        menus.state = state;
        menus.language = Some(language);
        cx.set_menus(commands::build_menus(cx));
    }
    pub(super) fn show_adjacent_tab(
        &mut self,
        next: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.tabs.len() + usize::from(self.home.is_some());
        if count < 2 {
            return;
        }
        let current = if self.home_in_front() {
            self.tabs.len()
        } else {
            self.active_tab
        };
        let target = if next {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        if target == self.tabs.len() {
            self.open_home_tab(window, cx);
        } else {
            let left = self.send_home_back();
            self.return_to_tab(target, left, cx);
        }
    }
}
/// Raise only main windows; utility hosts are never registered here.
pub(crate) fn bring_all_to_front(cx: &mut App) {
    let targets = activation_order(cx);
    if targets.is_empty() {
        return;
    }
    // Defer so the invoking window is no longer on GPUI's update stack.
    cx.defer(move |cx| {
        for target in targets {
            let _ = target.update(cx, |_, window, _| window.activate_window());
        }
    });
}
fn activation_order(cx: &App) -> Vec<AnyWindowHandle> {
    let Some(menus) = cx.try_global::<WindowMenus>() else {
        return Vec::new();
    };
    let stack = cx.window_stack();
    let frontmost = stack
        .as_ref()
        .and_then(|stack| stack.first().copied())
        .or_else(|| cx.active_window());
    let mut targets = stack.unwrap_or_else(|| menus.main_windows.clone());
    targets.retain(|target| menus.main_windows.contains(target) && Some(*target) != frontmost);
    targets.reverse();
    // Preserve the invoking/frontmost window, including a Settings utility host.
    if let Some(frontmost) = frontmost {
        targets.push(frontmost);
    }
    targets
}

#[cfg(feature = "gui-e2e")]
pub fn bring_all_activation_order(cx: &App) -> Vec<AnyWindowHandle> {
    activation_order(cx)
}
#[cfg(feature = "gui-e2e")]
pub fn main_window_targets(cx: &App) -> Vec<AnyWindowHandle> {
    cx.global::<WindowMenus>().main_windows.clone()
}
