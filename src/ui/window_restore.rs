//! Main repository-window geometry; secondary windows have separate owners.
use super::{settings, KagiApp};
use gpui::{
    point, px, size, App, Bounds, Context, DisplayId, Entity, Pixels, Task, Window, WindowBounds,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

fn enabled() -> bool {
    std::env::var("KAGI_NO_RESTORE").as_deref() != Ok("1") && std::env::var("KAGI_WINDOW").is_err()
}

fn frame_bounds(frame: &settings::MainWindowFrame) -> Bounds<Pixels> {
    Bounds {
        origin: point(px(frame.x), px(frame.y)),
        size: size(px(frame.width), px(frame.height)),
    }
}

/// Shared native options keep restored logical frames identical to saved frames.
pub fn options(cx: &App) -> gpui::WindowOptions {
    let bounds = initial_bounds(cx);
    let display_id = display_for_bounds(bounds, cx);
    // GPUI's macOS native host takes/returns display-local window origins;
    // persisted frames and connected-display intersection use desktop space.
    #[cfg(target_os = "macos")]
    let bounds = cx
        .displays()
        .iter()
        .find(|display| Some(display.id()) == display_id)
        .map_or(bounds, |display| bounds - display.bounds().origin);
    gpui::WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id,
        titlebar: super::main_window_titlebar(),
        app_owns_titlebar_drag: true,
        window_decorations: super::main_window_decorations(),
        // Wayland app_id / X11 WM_CLASS binds the shell to our desktop entry.
        app_id: Some(crate::APP_ID.to_string()),
        ..Default::default()
    }
}

pub fn initial_bounds(cx: &App) -> Bounds<Pixels> {
    if let Some((w, h)) = std::env::var("KAGI_WINDOW").ok().and_then(|raw| {
        let (w, h) = raw.split_once('x')?;
        Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
    }) {
        return Bounds::centered(None, size(px(w), px(h)), cx);
    }
    let frame = enabled().then(settings::main_window_frame).flatten();
    if let Some(frame) = &frame {
        let bounds = frame_bounds(frame);
        if cx
            .displays()
            .iter()
            .any(|display| bounds.intersects(&display.bounds()))
        {
            return bounds;
        }
    }
    let restored = frame.map(|frame| (frame.width, frame.height)).or_else(|| {
        enabled()
            .then(|| settings::Settings::load().window_size())
            .flatten()
            .filter(|(w, h)| w.is_finite() && h.is_finite() && *w >= 400. && *h >= 300.)
    });
    let ((w, h), (min_w, min_h)) = match restored {
        Some(wh) => (wh, (400_f32, 300_f32)),
        None => ((1440., 920.), (900_f32, 600_f32)),
    };
    let (w, h) = match cx.primary_display() {
        Some(display) => {
            let bounds = display.bounds();
            let max_w = f32::from(bounds.size.width) * 0.92;
            let max_h = f32::from(bounds.size.height) * 0.90;
            (
                w.clamp(min_w.min(max_w), max_w),
                h.clamp(min_h.min(max_h), max_h),
            )
        }
        None => (w, h),
    };
    Bounds::centered(None, size(px(w), px(h)), cx)
}

pub fn display_for_bounds(bounds: Bounds<Pixels>, cx: &App) -> Option<DisplayId> {
    let saved = enabled().then(settings::main_window_frame).flatten();
    let displays = cx.displays();
    let visible = |display: &&Rc<dyn gpui::PlatformDisplay>| bounds.intersects(&display.bounds());
    displays
        .iter()
        .filter(visible)
        .find(|display| {
            saved.as_ref().is_some_and(|frame| {
                display
                    .uuid()
                    .is_ok_and(|uuid| uuid.to_string() == frame.display)
            })
        })
        .or_else(|| displays.iter().find(visible))
        .map(|display| display.id())
}

fn capture(window: &Window, cx: &App) -> Option<settings::MainWindowFrame> {
    let WindowBounds::Windowed(bounds) = window.window_bounds() else {
        return None;
    };
    let display = window.display(cx);
    #[cfg(target_os = "macos")]
    let bounds = display
        .as_ref()
        .map_or(bounds, |display| bounds + display.bounds().origin);
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
