//! #1118: the reader owns the minimum width; the graph companion reveals
//! over it. #1790: the PR navigator is the shell's sidebar, not a companion.
//! Only presentation flags change. The session's PR tabs, inputs and lists stay
//! mounted in the same center and no read is admitted by a layout transition.
use super::{
    button_style::{apply_role, button_accessibility, named_button, ButtonRole},
    i18n::Msg,
    theme, KagiApp,
};
use gpui::{div, prelude::*, px, rgb, Context, Window};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    Disableable as _, Icon, Selectable as _, Sizable as _,
};
use kagi_ui_core::tooltip::HoverTooltip as _;

pub(super) const CENTER_MIN: f32 = 420.;
const TOOLBAR_H: f32 = 32.;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Companion {
    Swimlane,
}

pub(super) struct Layout {
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
        let lane_width = theme::scaled_px(super::pr_lane::MAX_PANE_WIDTH + 4.);
        let has_tab = app.pr_mode().is_some_and(|m| m.active.is_some());
        let lane_fits = width
            >= center
                + lane_width
                + if app.sidebar.visible && !compact {
                    theme::scaled_px(app.pane_fit.sidebar + 4.)
                } else {
                    px(0.)
                };
        let swimlane = lane_fits
            && app
                .pr_mode()
                .is_some_and(|m| m.active.is_some() && !m.lane_hidden);
        let drawer = app
            .pr_mode()
            .and_then(|m| m.companion)
            .filter(|_| has_tab && !lane_fits);
        Self {
            swimlane,
            drawer,
            lane_fits,
        }
    }
}
impl KagiApp {
    pub(super) fn toggle_pr_swimlane(&mut self, window: &Window) {
        let layout = Layout::for_window(self, window);
        if let Some(mode) = self.pr_mode_mut() {
            if layout.lane_fits {
                mode.lane_hidden = !mode.lane_hidden;
                mode.companion = None;
            } else {
                mode.companion = match mode.companion {
                    Some(Companion::Swimlane) => None,
                    None => Some(Companion::Swimlane),
                };
            }
        }
    }

    /// What the sidebar toggle would hide: the conflict tree or the shell's
    /// sidebar, which every workspace mode shares (#1790).
    pub(super) fn sidebar_shown(&self, cx: &gpui::App) -> bool {
        if self.conflict_body_visible() {
            if let Some(view) = self.ui().conflict.as_ref() {
                return !view.read(cx).sidebar_hidden;
            }
        }
        self.sidebar.visible
    }
}

/// #1790: the one sidebar toggle sits right after the window controls in every
/// mode (HIG Toolbars: show/hide sidebar belongs at the far leading edge), so
/// it never moves when the mode or tab changes. Home has no sidebar: disabled.
/// Compact windows already have one navigator launcher in the permanent rail.
pub(super) fn sidebar_toggle(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let disabled = app.home_in_front() || app.tabs.is_empty();
    let shown = !disabled && app.sidebar_shown(cx);
    let label = if shown {
        Msg::MenuHideSidebar.t()
    } else {
        Msg::MenuShowSidebar.t()
    };
    let shortcut = super::commands::effective_keystroke("view.toggleSidebar")
        .map(|s| super::commands::display_keystroke(&s))
        .unwrap_or_default();
    let help = gpui::SharedString::from(format!("{label} {shortcut}").trim_end().to_owned());
    let owner = app.active_session();
    super::e2e::measure_control(
        "sidebar-toggle",
        button_accessibility(
            Button::new("sidebar-toggle")
                .ghost()
                // Fixed like the traffic lights it follows; draws an 18 pt icon.
                .with_size(gpui::px(24.))
                .icon(Icon::empty().path("icons/sidebar-left.svg"))
                .disabled(disabled)
                .on_click(cx.listener(
                    move |this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
                        if this.active_session() == owner {
                            this.handle_menu_command("view.toggleSidebar", window, cx);
                        }
                    },
                )),
            label.into(),
            Some(shown),
            None,
            disabled,
        )
        .hover_tooltip("sidebar-toggle-help", help),
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
            apply_role(
                Button::new(id)
                    .icon(Icon::empty().path(icon))
                    .when(!label.is_empty(), |button| button.child(label)),
                ButtonRole::Secondary,
                cx,
            )
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
            Companion::Swimlane => "pr-companion-swimlane",
        }))
        .into_any_element()
}
