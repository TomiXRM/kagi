//! Main repository-window geometry; secondary windows have separate owners.
use super::{settings, KagiApp};
use gpui::{
    point, px, size, App, Bounds, Context, DisplayId, Entity, Pixels, Task, Window, WindowBounds,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

/// Smallest supported repository viewport, including compact chrome at 167% zoom.
pub fn minimum_size() -> gpui::Size<Pixels> {
    size(px(940.), px(660.))
}

fn enabled() -> bool {
    std::env::var("KAGI_NO_RESTORE").as_deref() != Ok("1") && std::env::var("KAGI_WINDOW").is_err()
}

fn frame_bounds(frame: &settings::MainWindowFrame) -> Bounds<Pixels> {
    Bounds {
        origin: point(px(frame.x), px(frame.y)),
        size: size(px(frame.width), px(frame.height)),
    }
}

/// Shared options keep display choice and geometry in one restore decision.
pub fn options(cx: &App) -> gpui::WindowOptions {
    let (bounds, display_id) = initial_placement(cx);
    #[cfg(not(target_os = "macos"))]
    let bounds = cx
        .displays()
        .iter()
        .find(|display| Some(display.id()) == display_id)
        .map_or(bounds, |display| bounds + display.bounds().origin);
    gpui::WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(minimum_size()),
        display_id,
        titlebar: super::main_window_titlebar(),
        app_owns_titlebar_drag: true,
        window_decorations: super::main_window_decorations(),
        // Wayland app_id / X11 WM_CLASS binds the shell to our desktop entry.
        app_id: Some(crate::APP_ID.to_string()),
        // Exercise the real Dock/reopen path without foregrounding native E2E windows.
        #[cfg(feature = "gui-e2e")]
        show: std::env::var_os("KAGI_GUI_E2E").is_none(),
        #[cfg(feature = "gui-e2e")]
        focus: std::env::var_os("KAGI_GUI_E2E").is_none(),
        ..Default::default()
    }
}

fn visible_bounds(display: &dyn gpui::PlatformDisplay) -> Bounds<Pixels> {
    let visible = display.visible_bounds();
    #[cfg(not(target_os = "macos"))]
    let visible = visible - display.bounds().origin;
    visible
}

/// UUID and a reachable titlebar qualify a frame before fitting its full rect.
pub fn restore_on_display(
    frame: &settings::MainWindowFrame,
    uuid: &str,
    visible: Bounds<Pixels>,
) -> Option<Bounds<Pixels>> {
    if frame.display.is_empty() || frame.display != uuid {
        return None;
    }
    let bounds = frame_bounds(frame);
    let titlebar_width = bounds.right().min(visible.right()) - bounds.left().max(visible.left());
    if bounds.top() < visible.top()
        || bounds.top() + px(28.) > visible.bottom()
        || titlebar_width < px(100.)
    {
        return None;
    }
    let minimum = minimum_size();
    let width = bounds.size.width.max(minimum.width).min(visible.size.width);
    let height = bounds
        .size
        .height
        .max(minimum.height)
        .min(visible.size.height);
    Some(Bounds {
        origin: point(
            bounds
                .origin
                .x
                .clamp(visible.left(), visible.right() - width),
            bounds
                .origin
                .y
                .clamp(visible.top(), visible.bottom() - height),
        ),
        size: size(width, height),
    })
}

fn initial_placement(cx: &App) -> (Bounds<Pixels>, Option<DisplayId>) {
    let primary = cx.primary_display();
    if let Some((w, h)) = std::env::var("KAGI_WINDOW").ok().and_then(|raw| {
        let (w, h) = raw.split_once('x')?;
        Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
    }) {
        let minimum = minimum_size();
        let bounds = Bounds::centered(
            None,
            size(px(w).max(minimum.width), px(h).max(minimum.height)),
            cx,
        );
        #[cfg(not(target_os = "macos"))]
        let bounds = primary
            .as_ref()
            .map_or(bounds, |display| bounds - display.bounds().origin);
        return (bounds, primary.map(|display| display.id()));
    }
    let frame = enabled().then(settings::main_window_frame).flatten();
    if let Some(frame) = &frame {
        for display in cx.displays() {
            let Ok(uuid) = display.uuid() else {
                continue;
            };
            if let Some(bounds) =
                restore_on_display(frame, &uuid.to_string(), visible_bounds(display.as_ref()))
            {
                return (bounds, Some(display.id()));
            }
        }
    }
    let restored = frame.map(|frame| (frame.width, frame.height)).or_else(|| {
        enabled()
            .then(|| settings::Settings::load().window_size())
            .flatten()
            .filter(|(w, h)| {
                w.is_finite()
                    && h.is_finite()
                    && (1. ..=16384.).contains(w)
                    && (1. ..=16384.).contains(h)
            })
    });
    let (w, h) = restored.unwrap_or((1440., 920.));
    let minimum = minimum_size();
    let (min_w, min_h) = (f32::from(minimum.width), f32::from(minimum.height));
    let Some(primary) = primary else {
        return (
            Bounds::centered(None, size(px(w.max(min_w)), px(h.max(min_h))), cx),
            None,
        );
    };
    let visible = visible_bounds(primary.as_ref());
    let (max_w, max_h) = (
        f32::from(visible.size.width),
        f32::from(visible.size.height),
    );
    let width = px(w.clamp(min_w.min(max_w), max_w));
    let height = px(h.clamp(min_h.min(max_h), max_h));
    (
        Bounds {
            origin: point(
                (visible.left() + (visible.size.width - width) / 2.).round(),
                (visible.top() + (visible.size.height - height) / 2.).round(),
            ),
            size: size(width, height),
        },
        Some(primary.id()),
    )
}

fn capture(window: &Window, cx: &App) -> Option<settings::MainWindowFrame> {
    let WindowBounds::Windowed(bounds) = window.window_bounds() else {
        return None;
    };
    let display = window.display(cx);
    #[cfg(not(target_os = "macos"))]
    let bounds = display
        .as_ref()
        .map_or(bounds, |display| bounds - display.bounds().origin);
    Some(settings::MainWindowFrame {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
        display: display
            .and_then(|display| display.uuid().ok())
            .map(|uuid| uuid.to_string())
            .unwrap_or_default(),
    })
}

fn save_pending(pending: &RefCell<Option<settings::MainWindowFrame>>) {
    if let Some(frame) = pending.borrow_mut().take() {
        settings::write_main_window_frame(&frame);
    }
}

/// Bounds notifications include moves without requiring another content render.
/// Retain one cancellable trailing task per main window, not a polling thread.
pub fn observe(entity: &Entity<KagiApp>, window: &mut Window, cx: &mut App) {
    if !enabled() {
        return;
    }
    entity.update(cx, |_, cx: &mut Context<KagiApp>| {
        let pending = Rc::new(RefCell::new(capture(window, cx)));
        let quit_pending = Rc::downgrade(&pending);
        cx.on_app_quit(move |_, _| {
            if let Some(pending) = quit_pending.upgrade() {
                save_pending(&pending);
            }
            settings::flush();
            async {}
        })
        .detach();
        let release_pending = pending.clone();
        cx.on_release(move |_, _| save_pending(&release_pending))
            .detach();
        let mut task: Option<Task<()>> = None;
        cx.observe_window_bounds(window, move |_, window, cx| {
            let Some(frame) = capture(window, cx) else {
                return;
            };
            *pending.borrow_mut() = Some(frame);
            let pending = pending.clone();
            drop(task.take());
            task = Some(cx.spawn(async move |_, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                save_pending(&pending);
            }));
        })
        .detach();
    });
}
