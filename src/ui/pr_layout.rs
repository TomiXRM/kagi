//! #1118: the reader owns the minimum width; companions reveal over it.
//! Only presentation flags change. The session's PR tabs, inputs and lists stay
//! mounted in the same center and no read is admitted by a layout transition.
use super::{
    button_style::{ButtonRole, KagiButton},
    i18n::Msg,
    theme, KagiApp,
};
use gpui::{div, prelude::*, px, rgb, Context, Window};
use gpui_component::{Selectable as _, Sizable as _};

pub(super) const CENTER_MIN: f32 = 420.;
const TOOLBAR_H: f32 = 32.;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Companion {
    Navigator,
    Swimlane,
}

pub(super) struct Layout {
    pub navigator: bool,
    pub swimlane: bool,
    lane_fits: bool,
}
impl Layout {
    pub fn for_window(app: &KagiApp, window: &Window) -> Self {
        let width = window.viewport_size().width;
        let center = theme::scaled_px(CENTER_MIN);
        let navigator_width = theme::scaled_px(app.sidebar.width + 4.);
        let lane_width = theme::scaled_px(super::pr_lane::MAX_PANE_WIDTH + 4.);
        let navigator = app.sidebar.visible && width >= center + navigator_width;
        let lane_fits = width
            >= center
                + lane_width
                + if app.sidebar.visible {
                    navigator_width
                } else {
                    px(0.)
                };
        let swimlane = lane_fits
            && app
                .pr_mode()
                .is_some_and(|m| m.active.is_some() && !m.lane_hidden);
        Self {
            navigator,
            swimlane,
            lane_fits,
        }
    }
}
impl KagiApp {
    pub(super) fn toggle_pr_companion(&mut self, pane: Companion, window: &Window) {
        let layout = Layout::for_window(self, window);
        let navigator_fits =
            window.viewport_size().width >= theme::scaled_px(CENTER_MIN + self.sidebar.width + 4.);
        if pane == Companion::Navigator {
            // The global View preference is still authoritative. Compact reveal
            // never flips it behind the menu's back.
            self.sidebar.visible = !(layout.navigator
                || (self.sidebar.visible
                    && self.pr_mode().is_some_and(|m| m.companion == Some(pane))));
        }
        let sidebar_visible = self.sidebar.visible;
        if let Some(mode) = self.pr_mode_mut() {
            if pane == Companion::Navigator && (!sidebar_visible || navigator_fits) {
                mode.companion = None;
                return;
            }
            if pane == Companion::Swimlane && layout.lane_fits {
                mode.lane_hidden = !mode.lane_hidden;
                mode.companion = None;
            } else {
                mode.companion = if mode.companion == Some(pane) {
                    None
                } else {
                    Some(pane)
                };
            }
        }
    }
}

pub(super) fn toolbar(
    app: &KagiApp,
    window: &Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let owner = app.active_session();
    let layout = Layout::for_window(app, window);
    let companion = app.pr_mode().and_then(|m| m.companion);
    let button = |id,
                  icon,
                  label: &'static str,
                  command: &'static str,
                  selected: bool,
                  cx: &mut Context<KagiApp>| {
        super::e2e::measure_control(
            id,
            KagiButton::icon(id, icon, label, ButtonRole::Neutral, cx)
                .small()
                .selected(selected)
                .tooltip(label)
                .on_click(cx.listener(
                    move |this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
                        if this.active_session() == owner {
                            this.handle_menu_command(command, window, cx);
                        }
                    },
                )),
        )
    };
    div()
        .h(theme::scaled_px(TOOLBAR_H))
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .flex_shrink_0()
        .bg(rgb(theme::theme().panel))
        .child(button(
            "pr-navigator-toggle",
            "icons/git-pull-request.svg",
            Msg::PrNavigator.t(),
            "view.toggleSidebar",
            layout.navigator || (app.sidebar.visible && companion == Some(Companion::Navigator)),
            cx,
        ))
        .child(button(
            "pr-swimlane-toggle",
            "icons/git-branch.svg",
            Msg::PrSwimlane.t(),
            "view.togglePrSwimlane",
            layout.swimlane || companion == Some(Companion::Swimlane),
            cx,
        ))
        .into_any_element()
}

pub(super) fn drawer(pane: gpui::AnyElement) -> gpui::AnyElement {
    div()
        .id("pr-companion-drawer")
        .absolute()
        .left_0()
        .top(theme::scaled_px(TOOLBAR_H))
        .bottom_0()
        .occlude()
        .max_w(gpui::relative(1.))
        .overflow_x_scroll()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(rgb(theme::theme().selected))
        .child(pane)
        .child(super::e2e::measure_inside("pr-companion-drawer"))
        .into_any_element()
}
