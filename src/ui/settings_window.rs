//! macOS non-modal Settings host (ADR-0223). Preferences and controls stay on
//! the existing KagiApp owner; this owns only window presentation and panes.
use super::{commands, i18n::Msg, keyboard_nav, settings, settings_view, theme, KagiApp};
use gpui::prelude::*;
use gpui::{
    div, point, px, size, App, AppContext, Bounds, Context, Entity, FocusHandle, Global,
    MouseButton, Render, ScrollHandle, Subscription, Window, WindowBounds, WindowHandle,
    WindowOptions,
};
use gpui_component::Root;

#[derive(Default)]
struct SettingsWindowHandle(Option<WindowHandle<Root>>);
impl Global for SettingsWindowHandle {}

pub(super) fn open_or_raise(owner: Entity<KagiApp>, cx: &mut App) {
    let existing = cx.default_global::<SettingsWindowHandle>().0;
    if let Some(handle) = existing {
        if handle.update(cx, |_, window, _| raise(window)).is_ok() {
            return;
        }
    }
    let options = window_options(cx);
    let error_owner = owner.clone();
    match cx.open_window(options, |window, cx| {
        owner.update(cx, |app, cx| {
            app.ensure_analyze_ignore_input(window, cx);
            app.refresh_smart_commit_detection(cx);
            if let Some(select) = &app.theme_select {
                let slug = theme::theme().slug.clone().into();
                select.update(cx, |state, cx| state.set_selected_value(&slug, window, cx));
            }
        });
        let host = cx.new(|cx| SettingsWindow::new(owner, window, cx));
        cx.new(|cx| Root::new(host, window, cx).font(theme::ui_font()))
    }) {
        Ok(handle) => cx.default_global::<SettingsWindowHandle>().0 = Some(handle),
        Err(error) => {
            eprintln!("Settings window: {error}");
            error_owner.update(cx, |app, cx| {
                app.push_toast(super::ToastKind::Error, Msg::SettingsWindowFailed.t(), cx);
            });
            cx.default_global::<SettingsWindowHandle>().0 = None;
        }
    }
}

fn window_options(cx: &App) -> WindowOptions {
    let height = cx
        .primary_display()
        .map_or(720., |display| {
            f32::from(display.bounds().size.height) * 0.85
        })
        .min(720.);
    let mut bounds = Bounds::centered(None, size(px(715.), px(height)), cx);
    if let Some((x, y)) = settings::read_setting("settings_window_position")
        .and_then(|value| {
            value
                .split_once(',')
                .and_then(|(x, y)| Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?)))
        })
        .filter(|(x, y)| x.is_finite() && y.is_finite())
    {
        let restored = point(px(x), px(y));
        if cx
            .displays()
            .iter()
            .any(|display| display.bounds().contains(&restored))
        {
            bounds.origin = restored;
        }
    }
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(Msg::SettingsTitle.t().into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(9.), px(16.))),
        }),
        app_owns_titlebar_drag: true,
        is_resizable: false,
        is_minimizable: true,
        #[cfg(feature = "gui-e2e")]
        show: std::env::var_os("KAGI_GUI_E2E").is_none(),
        #[cfg(feature = "gui-e2e")]
        focus: std::env::var_os("KAGI_GUI_E2E").is_none(),
        ..Default::default()
    }
}

struct SettingsWindow {
    owner: Entity<KagiApp>,
    pane: usize,
    focus: FocusHandle,
    tabs: keyboard_nav::TabFocus,
    scroll: ScrollHandle,
    _observe: Subscription,
}

impl SettingsWindow {
    fn new(owner: Entity<KagiApp>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pane = settings::read_setting("settings_pane")
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|&pane| pane < 4)
            .unwrap_or(0);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        window.on_window_should_close(cx, |window, _| {
            remember_position(window);
            settings::flush();
            true
        });
        cx.observe_window_bounds(window, |_, window, _| remember_position(window))
            .detach();
        let observe = cx.observe(&owner, |_, _, cx| cx.notify());
        Self {
            owner,
            pane,
            focus,
            tabs: Default::default(),
            scroll: Default::default(),
            _observe: observe,
        }
    }

    fn pane_tabs(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let host = cx.entity().downgrade();
        let tabs = keyboard_nav::TabList::new(
            &self.tabs,
            vec![0, 1, 2, 3],
            vec![0, 1, 2, 3],
            Some(self.pane),
            keyboard_nav::Activation::Automatic,
            Some(self.focus.clone()),
            move |pane, _, _, cx| {
                let _ = host.update(cx, |this, cx| {
                    this.pane = pane;
                    this.scroll.set_offset(Default::default());
                    settings::write_setting("settings_pane", Some(&pane.to_string()));
                    cx.notify();
                });
            },
            cx,
        );
        let labels = [
            Msg::SettingsGeneral,
            Msg::SettingsLanguage,
            Msg::SettingsSmartCommit,
            Msg::SettingsAnalyze,
        ];
        tabs.list(
            div()
                .id("settings-panes")
                .flex()
                .justify_center()
                .gap_1()
                .py_2(),
        )
        .children(labels.into_iter().enumerate().map(|(pane, msg)| {
            let label = msg.t();
            super::e2e::measure_control(
                [
                    "settings-pane-0",
                    "settings-pane-1",
                    "settings-pane-2",
                    "settings-pane-3",
                ][pane],
                tabs.cell(
                    pane,
                    label,
                    div()
                        .id(("settings-pane", pane))
                        .px_3()
                        .py_1()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_color(gpui::rgb(theme::theme().text_main))
                        .hover(|el| el.bg(gpui::rgb(theme::theme().surface)))
                        .when(self.pane == pane, |el| {
                            el.bg(gpui::rgb(theme::theme().surface))
                        })
                        .child(label),
                ),
            )
        }))
    }

    fn page(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let app = self.owner.read(cx);
        match self.pane {
            0 => settings_view::appearance_section(
                &self.owner,
                app.theme_select.clone(),
                &app.settings_switches,
                cx,
            )
            .into_any_element(),
            1 => settings_view::language_section(&self.owner).into_any_element(),
            2 => settings_view::smart_commit_section(
                &self.owner,
                &app.smart_commit,
                &app.settings_switches,
                cx,
            )
            .into_any_element(),
            _ => {
                settings_view::analyze_ignore_section(&self.owner, app.analyze_ignore_input.clone())
                    .into_any_element()
            }
        }
    }

    fn page_body(&self, cx: &Context<Self>) -> gpui::Div {
        let chrome = 46. + f32::from(theme::scaled_px(48.));
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .px_5()
            .py_4()
            .child(self.page(cx))
            .child(
                gpui::canvas(
                    move |bounds, window, cx| {
                        let max = window
                            .display(cx)
                            .map_or(720., |d| f32::from(d.bounds().size.height) * 0.85);
                        let height =
                            (f32::from(bounds.size.height) + chrome).clamp(500_f32.min(max), max);
                        if (f32::from(window.viewport_size().height) - height).abs() > 1. {
                            window.resize(size(px(715.), px(height)));
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        keyboard_nav::forget_roving_without_focus(window);
        window.set_window_title(Msg::SettingsTitle.t());
        window.set_rem_size(theme::scaled_px(16.));
        let tabs = self.pane_tabs(cx);
        let body = self.page_body(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(gpui::rgb(theme::theme().panel))
            .font_family(super::UI_FONT)
            .text_color(gpui::rgb(theme::theme().text_main))
            .track_focus(&self.focus)
            .on_action(|_: &commands::CloseTab, window, _| close(window))
            .on_action(|_: &commands::OpenSettings, window, _| raise(window))
            .on_action(|_: &commands::MinimizeWindow, window, _| window.minimize_window())
            .on_action(|_: &commands::CloseWindow, window, _| close(window))
            .on_action(cx.listener(|this, _: &commands::Quit, window, cx| {
                if this.owner.update(cx, |app, cx| !app.hold_host_close(cx)) {
                    remember_position(window);
                    settings::flush();
                    cx.quit();
                }
            }))
            .child(titlebar())
            .child(tabs)
            .child(super::render_helpers::with_system_vertical_scrollbar(
                "settings-window-scrollbar",
                &self.scroll,
                div()
                    .id("settings-window-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(body),
                cx,
            ))
    }
}

fn remember_position(window: &Window) {
    let origin = window.bounds().origin;
    settings::write_setting(
        "settings_window_position",
        Some(&format!("{},{}", f32::from(origin.x), f32::from(origin.y))),
    );
}

fn raise(window: &mut Window) {
    // Native E2E windows stay hidden and never take the user's foreground.
    // OS ordering/activation is exercised by the PID-isolated Tier B controller.
    #[cfg(feature = "gui-e2e")]
    if std::env::var_os("KAGI_GUI_E2E").is_some() {
        window.refresh();
        return;
    }
    window.activate_window();
}

fn close(window: &mut Window) {
    remember_position(window);
    settings::flush();
    window.remove_window();
}

fn titlebar() -> gpui::Div {
    div()
        .h(px(46.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .pl(px(80.))
        .child(
            div()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .pr(px(80.))
                .child(Msg::SettingsTitle.t())
                .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
        )
}
