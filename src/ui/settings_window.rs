//! macOS non-modal Settings host (ADR-0223). Preferences and controls stay on
//! the existing KagiApp owner; this owns only window presentation and panes.
use super::{commands, i18n::Msg, settings, theme, KagiApp};
use gpui::prelude::*;
use gpui::{
    div, point, px, size, App, AppContext, Bounds, Context, Entity, FocusHandle, Global,
    MouseButton, Render, Subscription, WeakEntity, Window, WindowBounds, WindowHandle,
    WindowOptions,
};
use gpui_component::Root;

const SETTINGS_WIDTH: f32 = 900.;
const SETTINGS_HEIGHT: f32 = 760.;

#[derive(Default)]
struct SettingsWindowHandle(Option<WindowHandle<Root>>);
impl Global for SettingsWindowHandle {}

#[cfg(feature = "gui-e2e")]
pub(super) fn view_for_window(
    window: &Window,
    cx: &App,
) -> Option<Entity<kagi_ui_settings::SettingsView>> {
    window
        .root::<Root>()
        .flatten()
        .and_then(|root| {
            root.read(cx)
                .view()
                .clone()
                .downcast::<SettingsWindow>()
                .ok()
        })
        .map(|host| host.read(cx).view.clone())
}

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
    let max_height = visible.map_or(SETTINGS_HEIGHT, |bounds| f32::from(bounds.size.height));
    let height = saved
        .map_or(SETTINGS_HEIGHT, |frame| frame.height)
        .clamp(500_f32.min(max_height), max_height);
    let width = visible.map_or(px(SETTINGS_WIDTH), |bounds| {
        px(SETTINGS_WIDTH).min(bounds.size.width)
    });
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
    view: Entity<kagi_ui_settings::SettingsView>,
    focus: FocusHandle,
    input_escape_started_marked: bool,
    last_fit: Option<(f32, kagi_ui_settings::Pane, f32, u64)>,
    _observe: Subscription,
    _release: Subscription,
    _toast_removed: Option<Subscription>,
}

impl SettingsWindow {
    fn new(owner: Entity<KagiApp>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let view = owner
            .read(cx)
            .settings_entity()
            .expect("Settings initialized before native host");
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        let host = cx.entity().downgrade();
        view.update(cx, |view, cx| {
            view.set_content_measure(
                std::rc::Rc::new(move |bounds, window, cx| {
                    let _ = host.update(cx, |host, cx| {
                        let height = f32::from(bounds.size.height);
                        let pane = host.view.read(cx).pane();
                        let zoom = theme::zoom();
                        let displays = display_configuration();
                        if host.last_fit.is_some_and(|previous| {
                            (previous.0 - height).abs() < 0.5
                                && previous.1 == pane
                                && previous.2 == zoom
                                && previous.3 == displays
                        }) {
                            return;
                        }
                        host.last_fit = Some((height, pane, zoom, displays));
                        fit_content(window, height, cx);
                    });
                }),
                cx,
            )
        });
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
            view,
            focus,
            input_escape_started_marked: false,
            last_fit: None,
            _observe: observe,
            _release: release,
            _toast_removed: toast_removed,
        }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        super::window_menu::publish_settings_edit_state(window, cx);
        let Some(owner) = self.owner.upgrade() else {
            close(window, cx);
            return div().into_any_element();
        };
        let pane = self.view.read(cx).pane();
        window.set_window_title(&format!("{} — {}", Msg::SettingsTitle.t(), pane.title()));
        window.set_rem_size(theme::scaled_px(16.));

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
            .on_mouse_down(MouseButton::Left, |event, window, _| {
                // The canonical sidebar reserves its top inset for native
                // window controls and dragging, not a second painted titlebar.
                if event.position.y < theme::scaled_px(40. * 16. / theme::BASE_REM_PX) {
                    window.start_window_move();
                }
            })
            .child(self.view.clone())
            .when_some(
                owner
                    .read(cx)
                    .analyze_reset_alert()
                    .filter(|alert| alert.window == window.window_handle().window_id())
                    .cloned(),
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
        fitted.size.width = (f64::from(SETTINGS_WIDTH) + current.size.width - content.size.width)
            .min(visible.size.width);
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
