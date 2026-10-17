//! #1118: the reader owns the minimum width; companions reveal over it.
//! Only presentation flags change. The session's PR tabs, inputs and lists stay
//! mounted in the same center and no read is admitted by a layout transition.
use super::{
    button_style::{named_button, ButtonRole, KagiButton},
    i18n::Msg,
    theme, KagiApp,
};
use gpui::{div, prelude::*, px, rgb, Context, Window};
use gpui_component::{Selectable as _, Sizable as _};
use kagi_ui_core::tooltip::HoverTooltip as _;

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
    pub drawer: Option<Companion>,
    lane_fits: bool,
}
impl Layout {
    pub fn for_window(app: &KagiApp, window: &Window) -> Self {
        let compact = super::compact::for_window(window);
        let width = window.viewport_size().width
            - if compact {
                theme::scaled_px(super::compact::RAIL_WIDTH)
            } else {
                px(0.)
            };
        let center = theme::scaled_px(CENTER_MIN);
        let navigator_width = theme::scaled_px(app.sidebar.width + 4.);
        let lane_width = theme::scaled_px(super::pr_lane::MAX_PANE_WIDTH + 4.);
        let has_tab = app.pr_mode().is_some_and(|m| m.active.is_some());
        let navigator =
            !compact && app.sidebar.visible && (!has_tab || width >= center + navigator_width);
        let lane_fits = width
            >= center
                + lane_width
                + if app.sidebar.visible && !compact {
                    navigator_width
                } else {
                    px(0.)
                };
        let swimlane = lane_fits
            && app
                .pr_mode()
                .is_some_and(|m| m.active.is_some() && !m.lane_hidden);
        let drawer = app.pr_mode().and_then(|m| m.companion).filter(|pane| {
            has_tab
                && match pane {
                    Companion::Navigator => !compact && app.sidebar.visible && !navigator,
                    Companion::Swimlane => !lane_fits,
                }
        });
        Self {
            navigator,
            swimlane,
            drawer,
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

/// The leading chrome control remains mounted when the sidebar is hidden.
/// Compact windows already have one navigator launcher in the permanent rail.
pub(super) fn navigator_toggle(
    app: &KagiApp,
    window: &Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let layout = Layout::for_window(app, window);
    let selected = layout.navigator || layout.drawer == Some(Companion::Navigator);
    companion_button(
        app,
        "pr-navigator-toggle",
        "icons/panel-left.svg",
        "",
        if selected {
            Msg::PrHideNavigator.t()
        } else {
            Msg::PrShowNavigator.t()
        },
        "view.toggleSidebar",
        selected,
        cx,
    )
}

#[allow(clippy::too_many_arguments)]
fn companion_button(
    app: &KagiApp,
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    help: &'static str,
    command: &'static str,
    selected: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let owner = app.active_session();
    super::e2e::measure_control(
        id,
        named_button(
            KagiButton::icon(id, icon, label, ButtonRole::Secondary, cx)
                .small()
                .selected(selected),
            help,
            Some(selected),
        )
        .on_click(cx.listener(
            move |this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
                if this.active_session() == owner {
                    this.handle_menu_command(command, window, cx);
                }
            },
        ))
        .hover_tooltip((id, 0usize), help),
    )
    .into_any_element()
}

pub(super) fn toolbar(
    app: &KagiApp,
    window: &Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let layout = Layout::for_window(app, window);
    let selected = layout.swimlane || layout.drawer == Some(Companion::Swimlane);
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
        .child(companion_button(
            app,
            "pr-swimlane-toggle",
            "icons/git-branch.svg",
            Msg::PrSwimlane.t(),
            if selected {
                Msg::PrHideGraph.t()
            } else {
                Msg::PrShowGraph.t()
            },
            "view.togglePrSwimlane",
            selected,
            cx,
        ))
        .into_any_element()
}

pub(super) fn drawer(pane: gpui::AnyElement, companion: Companion) -> gpui::AnyElement {
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
        .child(super::e2e::measure_inside(match companion {
            Companion::Navigator => "pr-companion-navigator",
            Companion::Swimlane => "pr-companion-swimlane",
        }))
        .into_any_element()
}
