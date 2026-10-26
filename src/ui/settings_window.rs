//! macOS non-modal Settings host (ADR-0223). Preferences and controls stay on
//! the existing KagiApp owner; this owns only window presentation and panes.
use super::{commands, i18n::Msg, keyboard_nav, settings, settings_view, theme, KagiApp};
use gpui::prelude::*;
use gpui::{
    div, point, px, size, App, AppContext, Bounds, Context, Entity, FocusHandle, Global,
    MouseButton, Render, ScrollHandle, Subscription, WeakEntity, Window, WindowBounds,
    WindowHandle, WindowOptions,
};
use gpui_component::Root;

#[derive(Default)]
struct SettingsWindowHandle(Option<WindowHandle<Root>>);
impl Global for SettingsWindowHandle {}

pub(super) fn owns_window(id: gpui::WindowId, cx: &App) -> bool {
    cx.try_global::<SettingsWindowHandle>()
        .and_then(|handle| handle.0)
        .is_some_and(|handle| handle.window_id() == id)
}

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
                app.push_toast_for(
                    super::toast_stack::ToastSubject::App,
                    super::ToastKind::Error,
                    Msg::SettingsWindowFailed.t(),
                    cx,
                );
            });
            cx.default_global::<SettingsWindowHandle>().0 = None;
        }
    }
}

fn window_options(cx: &App) -> WindowOptions {
    let saved = settings::settings_window_frame();
    let display = saved
        .and_then(|frame| frame.display_uuid)
        .and_then(|uuid| {
            cx.displays()
                .into_iter()
                .find(|display| display.uuid().is_ok_and(|id| id.as_u128() == uuid))
        })
        .or_else(|| cx.primary_display());
    let visible = display.as_ref().map(|display| display.visible_bounds());
    let max_height = visible.map_or(720., |bounds| f32::from(bounds.size.height));
    let height = saved
        .map_or(720., |frame| frame.height)
        .clamp(500_f32.min(max_height), max_height);
    let width = visible.map_or(px(715.), |bounds| px(715.).min(bounds.size.width));
    let mut bounds = Bounds::centered(None, size(width, px(height)), cx);
    if let Some(visible) = visible {
        let restore = saved.filter(|frame| {
            frame.display_uuid.is_none()
                || display.as_ref().is_some_and(|display| {
                    display
                        .uuid()
                        .is_ok_and(|uuid| Some(uuid.as_u128()) == frame.display_uuid)
                })
        });
        bounds.origin = restore.map_or_else(
            || visible.center() - (bounds.size / 2.).into(),
            |frame| point(px(frame.x), px(frame.y)),
        );
        bounds.origin.x = bounds
            .origin
            .x
            .clamp(visible.left(), visible.right() - width);
        bounds.origin.y = bounds
            .origin
            .y
            .clamp(visible.top(), visible.bottom() - px(height));
    }
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id: display.map(|display| display.id()),
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
    owner: WeakEntity<KagiApp>,
    pane: usize,
    focus: FocusHandle,
    input_escape_started_marked: bool,
    tabs: keyboard_nav::TabFocus,
    scroll: ScrollHandle,
    last_fit: Option<(f32, usize, f32, u64)>,
    _observe: Subscription,
    _release: Subscription,
    _toast_removed: Option<Subscription>,
}

impl SettingsWindow {
    fn new(owner: Entity<KagiApp>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pane = settings::read_setting("settings_pane")
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|&pane| pane < 4)
            .unwrap_or(0);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        window.on_window_should_close(cx, |window, cx| {
            remember_geometry(window, cx);
            settings::flush();
            true
        });
        cx.observe_window_bounds(window, |_, window, cx| {
            remember_geometry(window, cx);
        })
        .detach();
        let observe = cx.observe(&owner, |_, _, cx| cx.notify());
        let release = cx.observe_release_in(&owner, window, |_, _, window, cx| close(window, cx));
        let toast_removed = owner.read(cx).toast_stack.clone().map(|stack| {
            cx.observe(&stack, |_, _, cx| cx.notify()).detach();
            cx.subscribe_in(
                &stack,
                window,
                |this, _, removed: &super::toast_stack::ToastRemoved, window, cx| {
                    if removed.was_focused(window)
                        || window.focused(cx).is_none()
                        || !this.focus.contains_focused(window, cx)
                    {
                        this.focus.focus(window, cx);
                    }
                },
            )
        });
        let id = window.window_handle().window_id();
        cx.on_release(move |this, cx| {
            if let Some(stack) = this
                .owner
                .upgrade()
                .and_then(|owner| owner.read(cx).toast_stack.clone())
            {
                stack.update(cx, |stack, _| stack.forget_window(id));
            }
            if let Some(owner) = this.owner.upgrade() {
                owner.update(cx, |app, cx| {
                    let reset = app.clear_analyze_reset_alert(id);
                    let consent = app.clear_settings_smart_consent(id);
                    if reset || consent {
                        cx.notify();
                    }
                });
            }
        })
        .detach();
        Self {
            owner: owner.downgrade(),
            pane,
            focus,
            input_escape_started_marked: false,
            tabs: Default::default(),
            scroll: Default::default(),
            last_fit: None,
            _observe: observe,
            _release: release,
            _toast_removed: toast_removed,
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
                .flex_wrap()
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

    fn page(&self, owner: &Entity<KagiApp>, cx: &Context<Self>) -> gpui::AnyElement {
        let app = owner.read(cx);
        match self.pane {
            0 => settings_view::appearance_section(
                owner,
                app.theme_select.clone(),
                &app.settings_switches,
                &self.scroll,
                cx,
            )
            .into_any_element(),
            1 => settings_view::language_section(owner).into_any_element(),
            2 => settings_view::smart_commit_section(
                owner,
                &app.smart_commit,
                &app.settings_switches,
                &app.settings_controls,
                Some(self.focus.clone()),
                &self.scroll,
                cx,
            )
            .into_any_element(),
            _ => settings_view::analyze_ignore_section(
                owner,
                app.analyze_ignore_input.clone(),
                app.analyze_ignore_dirty(cx),
                app.settings_controls.ignore_error.as_deref(),
            )
            .into_any_element(),
        }
    }

    fn page_body(
        &self,
        owner: &Entity<KagiApp>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let chrome = 46. + f32::from(theme::scaled_px(48.));
        let host = cx.entity().downgrade();
        let pane = self.pane;
        let zoom = theme::zoom();
        let persistent_lane = owner.read(cx).toast_stack.as_ref().is_some_and(|stack| {
            stack
                .read(cx)
                .persistent_lane_open(super::toast_stack::ToastAudience::Settings)
        });
        let notifications = persistent_lane
            .then(|| {
                owner.update(cx, |app, cx| {
                    app.render_toasts(
                        window,
                        super::busy::ToastPlacement::PersistentLane,
                        crate::ui::toast_stack::ToastAudience::Settings,
                        cx,
                    )
                })
            })
            .flatten();
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .px_5()
            .py_4()
            .child(
                div()
                    .relative()
                    .flex()
                    .justify_end()
                    .flex_shrink_0()
                    .child(super::e2e::measure_inside("toast-content-anchor"))
                    .children(notifications),
            )
            .child(
                div()
                    .relative()
                    .child(super::e2e::measure_inside("settings-page-controls"))
                    .child(self.page(owner, cx)),
            )
            .child(
                gpui::canvas(
                    move |bounds, window, cx| {
                        let height = f32::from(bounds.size.height) + chrome;
                        let displays = display_configuration();
                        let _ = host.update(cx, |this, cx| {
                            if this.last_fit.is_some_and(|previous| {
                                (previous.0 - height).abs() < 0.5
                                    && previous.1 == pane
                                    && previous.2 == zoom
                                    && previous.3 == displays
                            }) {
                                return;
                            }
                            this.last_fit = Some((height, pane, zoom, displays));
                            fit_content(window, height, cx);
                        });
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
        super::window_menu::publish_settings_edit_state(window, cx);
        keyboard_nav::forget_roving_without_focus(window);
        let Some(owner) = self.owner.upgrade() else {
            close(window, cx);
            return div().into_any_element();
        };
        window.set_window_title(Msg::SettingsTitle.t());
        window.set_rem_size(theme::scaled_px(16.));
        let tabs = self.pane_tabs(cx);
        let body = self.page_body(&owner, window, cx);
        owner.update(cx, |app, cx| {
            let models_shown = self.pane == 2
                && matches!(
                    app.smart_commit.provider,
                    super::smart_commit::SmartProvider::Ollama
                )
                && !app.smart_commit.detected_models.is_empty();
            app.settings_controls.repair_focus(
                self.pane == 2,
                models_shown,
                Some(&self.focus),
                window,
                cx,
            );
        });
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(gpui::rgb(theme::theme().panel))
            .font_family(super::UI_FONT)
            .text_color(gpui::rgb(theme::theme().text_main))
            .track_focus(&self.focus)
            .capture_action(
                cx.listener(|this, _: &gpui_component::input::Escape, window, cx| {
                    this.input_escape_started_marked =
                        kagi_ui_core::input_actions::focused_input_has_marked_text(window, cx);
                    cx.propagate();
                }),
            )
            .on_action(
                cx.listener(|this, _: &gpui_component::input::Escape, _, cx| {
                    if !this.input_escape_started_marked {
                        cx.propagate();
                    }
                }),
            )
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.modifiers != gpui::Modifiers::none() {
                    return;
                }
                let direction = match event.keystroke.key.as_str() {
                    "pageup" => 1.,
                    "pagedown" => -1.,
                    _ => return,
                };
                let mut offset = this.scroll.offset();
                offset.y = (offset.y + this.scroll.bounds().size.height * direction)
                    .clamp(-this.scroll.max_offset().y, px(0.));
                this.scroll.set_offset(offset);
                window.refresh();
                cx.stop_propagation();
            }))
            .on_action(|_: &commands::CloseTab, window, cx| close(window, cx))
            .map(|el| commands::register_recent_clear(el, cx))
            .on_action(|_: &commands::OpenSettings, window, _| raise(window))
            .on_action(|_: &commands::MinimizeWindow, window, _| window.minimize_window())
            .on_action(|_: &commands::CloseWindow, window, cx| close(window, cx))
            .on_action(cx.listener(|this, _: &commands::Quit, window, cx| {
                if this
                    .owner
                    .update(cx, |app, cx| {
                        !app.hold_host_close(super::EditorPendingIntent::Quit, cx)
                    })
                    .unwrap_or(false)
                {
                    remember_geometry(window, cx);
                    settings::flush();
                    commands::quit_app(cx);
                } else {
                    let owner = this.owner.clone();
                    let settings_id = window.window_handle().window_id();
                    cx.defer(move |cx| {
                        for handle in cx.windows() {
                            if handle.window_id() == settings_id {
                                continue;
                            }
                            let _ = handle.update(cx, |_, window, cx| {
                                if super::repository_window(window, cx)
                                    .is_some_and(|app| app.downgrade() == owner)
                                {
                                    raise(window);
                                    window.refresh();
                                }
                            });
                        }
                    });
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
            .map(|el| {
                el.children(owner.update(cx, |app, cx| {
                    app.render_toasts(
                        window,
                        super::busy::ToastPlacement::Transient,
                        crate::ui::toast_stack::ToastAudience::Settings,
                        cx,
                    )
                }))
            })
            .when_some(
                owner.read(cx).analyze_reset_alert().cloned(),
                |el, modal| el.child(super::settings_analyze::reset_alert(&owner, modal, cx)),
            )
            .when_some(
                owner
                    .read(cx)
                    .settings_smart_consent()
                    .filter(|alert| alert.window == window.window_handle().window_id())
                    .cloned(),
                |el, alert| {
                    el.child(super::settings_controls::smart_consent_alert(
                        &owner, alert, cx,
                    ))
                },
            )
            .into_any_element()
    }
}

/// Check the whole display layout, not the window's current display: a user
/// dragging across a display boundary is not a configuration change. Native
/// enumeration avoids allocating GPUI's Vec of display wrappers every prepaint.
fn display_configuration() -> u64 {
    use objc2::{class, msg_send, runtime::AnyObject};
    use objc2_foundation::NSRect;
    use std::hash::{Hash, Hasher};

    let mut hash = std::collections::hash_map::DefaultHasher::new();
    // SAFETY: AppKit screens and their immutable rectangles are read only on
    // the main thread; none of the borrowed native objects escape this call.
    unsafe {
        let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
        let count: usize = msg_send![screens, count];
        count.hash(&mut hash);
        for index in 0..count {
            let screen: *mut AnyObject = msg_send![screens, objectAtIndex: index];
            let frame: NSRect = msg_send![screen, frame];
            let visible: NSRect = msg_send![screen, visibleFrame];
            for rect in [frame, visible] {
                for coordinate in [
                    rect.origin.x,
                    rect.origin.y,
                    rect.size.width,
                    rect.size.height,
                ] {
                    coordinate.to_bits().hash(&mut hash);
                }
            }
        }
    }
    hash.finish()
}

/// GPUI exposes content resizing but no native frame positioning. Apply both
/// together on the foreground executor, outside the drawing/window borrow.
fn fit_content(window: &Window, height: f32, cx: &App) {
    use objc2::{msg_send, rc::Retained, runtime::AnyObject};
    use objc2_foundation::NSRect;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: GPUI's AppKit handle is a live NSView, and this host is accessed
    // only on the main thread. Retain its exact NSWindow across the queued task.
    let target = unsafe {
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        let native: *mut AnyObject = msg_send![view, window];
        let screen: *mut AnyObject = msg_send![native, screen];
        if native.is_null() || screen.is_null() {
            return;
        }
        let current: NSRect = msg_send![native, frame];
        let content: NSRect = msg_send![view, frame];
        let visible: NSRect = msg_send![screen, visibleFrame];
        let inset = current.size.height - content.size.height;
        let max_content = (visible.size.height - inset).max(0.);
        let content_height = f64::from(height).clamp(500_f64.min(max_content), max_content);
        let mut fitted = current;
        fitted.size.width =
            (715. + current.size.width - content.size.width).min(visible.size.width);
        fitted.size.height = content_height + inset;
        // Preserve the top edge instead of Cocoa setContentSize's bottom anchor.
        fitted.origin.y += current.size.height - fitted.size.height;
        fitted.origin.x = fitted.origin.x.clamp(
            visible.origin.x,
            visible.origin.x + visible.size.width - fitted.size.width,
        );
        fitted.origin.y = fitted.origin.y.clamp(
            visible.origin.y,
            visible.origin.y + visible.size.height - fitted.size.height,
        );
        if (current.origin.x - fitted.origin.x).abs() < 0.5
            && (current.origin.y - fitted.origin.y).abs() < 0.5
            && (current.size.width - fitted.size.width).abs() < 0.5
            && (current.size.height - fitted.size.height).abs() < 0.5
        {
            return;
        }
        Retained::retain(native).map(|native| (native, fitted))
    };
    if let Some((native, fitted)) = target {
        cx.foreground_executor()
            .spawn(async move {
                // SAFETY: the retained NSWindow and this executor stay on the
                // main thread; AppKit delivers normal move/resize notifications.
                unsafe {
                    let _: () = msg_send![&*native, setFrame: fitted, display: true];
                }
            })
            .detach();
    }
}

fn remember_geometry(window: &Window, cx: &App) {
    let origin = window.bounds().origin;
    settings::write_settings_window_frame(settings::SettingsWindowFrame {
        x: f32::from(origin.x),
        y: f32::from(origin.y),
        height: f32::from(window.viewport_size().height),
        display_uuid: window
            .display(cx)
            .and_then(|display| display.uuid().ok())
            .map(|uuid| uuid.as_u128()),
    });
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

fn close(window: &mut Window, cx: &App) {
    remember_geometry(window, cx);
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
