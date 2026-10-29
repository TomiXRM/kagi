//! Native gallery host for Kagi parts and pages (#1799; experiment
//! `experiments/settings-skill`).
//!
//! A gallery binary (`examples/*.rs`) passes its pages to [`run`]. Everything a
//! capture depends on is chosen by environment, so a capture is reproducible:
//!
//! | Variable | Meaning (default) |
//! |---|---|
//! | `KAGI_GALLERY_PAGE` | page name (the first page) |
//! | `KAGI_GALLERY_STATE` | page state (`default`); unknown names fail |
//! | `KAGI_GALLERY_THEME` | theme slug (`apple-dark`); unknown slugs fail |
//! | `KAGI_GALLERY_LANG` | `en` / `ja` (`en`) |
//! | `KAGI_GALLERY_ZOOM` | 0.7–2.0 (`1.0`) |
//! | `KAGI_GALLERY_SIZE` | `<w>x<h>` logical window size (the page's size) |
//! | `KAGI_GALLERY_DISPLAY` | display UUID prefix to open on (primary) |
//! | `KAGI_GALLERY_OUT` | PNG path: capture the window, then exit |
//! | `KAGI_GALLERY_EXIT_AFTER_MS` | quit after this long (30000 with `OUT`) |
//!
//! `--list` prints `page<TAB>state` for every capture target.
//!
//! The host never takes the foreground: the window opens unfocused and the
//! application is never activated (#1799 comment: an unconditional
//! `activate` brought the demo forward). Settings writes go to a private
//! `KAGI_LOG_DIR` unless the caller set one. A capture whose window is not
//! visible on an active Space fails instead of saving a stale frame.
use gpui::{
    point, px, size, AnyElement, App, AppContext as _, Bounds, Context, IntoElement, Render,
    Window, WindowBounds, WindowOptions,
};
use gpui_component::Root;
use std::time::Duration;

use super::{fonts, i18n, theme};

/// What a page renders for: its state name and the active language.
pub struct GalleryContext {
    pub state: &'static str,
    pub lang: i18n::Lang,
}

/// One capture target. `states` lists every valid `KAGI_GALLERY_STATE`; the
/// first one is the default.
pub struct GalleryPage {
    pub name: &'static str,
    pub states: &'static [&'static str],
    /// Logical window size at 1.0× zoom.
    pub size: (f32, f32),
    pub render: fn(&GalleryContext, &mut Window, &mut App) -> AnyElement,
}

struct Selection {
    page: &'static GalleryPage,
    state: &'static str,
    size: (f32, f32),
    display: Option<String>,
    out: Option<std::path::PathBuf>,
    exit_after: Option<Duration>,
}

fn fail(message: String) -> ! {
    eprintln!("gallery: {message}");
    std::process::exit(2);
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn select(pages: &'static [GalleryPage]) -> Selection {
    let page = match env("KAGI_GALLERY_PAGE") {
        None => pages.first().unwrap_or_else(|| fail("no pages".into())),
        Some(name) => pages.iter().find(|p| p.name == name).unwrap_or_else(|| {
            let names: Vec<_> = pages.iter().map(|p| p.name).collect();
            fail(format!(
                "unknown page {name:?}; pages: {}",
                names.join(", ")
            ))
        }),
    };
    let state = match env("KAGI_GALLERY_STATE") {
        None => page.states.first().copied().unwrap_or("default"),
        Some(name) => page
            .states
            .iter()
            .copied()
            .find(|s| *s == name)
            .unwrap_or_else(|| {
                fail(format!(
                    "unknown state {name:?} for page {}; states: {}",
                    page.name,
                    page.states.join(", ")
                ))
            }),
    };
    let size = match env("KAGI_GALLERY_SIZE") {
        None => page.size,
        Some(raw) => raw
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
            .unwrap_or_else(|| fail(format!("KAGI_GALLERY_SIZE {raw:?} is not <w>x<h>"))),
    };
    let out = env("KAGI_GALLERY_OUT").map(std::path::PathBuf::from);
    let exit_after = env("KAGI_GALLERY_EXIT_AFTER_MS")
        .map(|ms| {
            ms.parse()
                .unwrap_or_else(|_| fail(format!("KAGI_GALLERY_EXIT_AFTER_MS {ms:?}")))
        })
        .or(out.as_ref().map(|_| 30_000))
        .map(Duration::from_millis);
    Selection {
        page,
        state,
        size,
        display: env("KAGI_GALLERY_DISPLAY").map(|d| d.to_ascii_lowercase()),
        out,
        exit_after,
    }
}

fn apply_environment() {
    if env("KAGI_LOG_DIR").is_none() {
        let dir = std::env::temp_dir().join(format!("kagi-gallery-{}", std::process::id()));
        if let Err(error) = std::fs::create_dir_all(&dir) {
            fail(format!("private settings dir {}: {error}", dir.display()));
        }
        std::env::set_var("KAGI_LOG_DIR", &dir);
    }
    theme::init_active();
    let slug = env("KAGI_GALLERY_THEME").unwrap_or_else(|| "apple-dark".into());
    if !theme::set_active(&slug) {
        fail(format!("unknown theme {slug:?}"));
    }
    let lang = env("KAGI_GALLERY_LANG").unwrap_or_else(|| "en".into());
    match i18n::Lang::from_slug(&lang) {
        Some(lang) => i18n::set_lang(lang),
        None => fail(format!("unknown language {lang:?} (en, ja)")),
    }
    let zoom = env("KAGI_GALLERY_ZOOM").unwrap_or_else(|| "1.0".into());
    match zoom.parse::<f32>() {
        Ok(z) if (theme::ZOOM_MIN..=theme::ZOOM_MAX).contains(&z) => {
            theme::set_zoom(z);
        }
        _ => fail(format!("KAGI_GALLERY_ZOOM {zoom:?} outside 0.7–2.0")),
    }
}

struct GalleryView {
    page: &'static GalleryPage,
    context: GalleryContext,
}

impl Render for GalleryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        (self.page.render)(&self.context, window, cx)
    }
}

/// Print every capture target, then exit, when the binary got `--list`.
fn list_and_exit_if_asked(pages: &'static [GalleryPage]) {
    if std::env::args().any(|arg| arg == "--list") {
        for page in pages {
            for state in page.states {
                println!("{}\t{}", page.name, state);
            }
        }
        std::process::exit(0);
    }
}

/// Open the selected page in a native window; capture and exit with `OUT`.
pub fn run(pages: &'static [GalleryPage]) {
    list_and_exit_if_asked(pages);
    let selection = select(pages);
    apply_environment();
    let application = gpui_platform::application().with_assets(super::assets::KagiAssets);
    application.run(move |cx: &mut App| {
        fonts::load_bundled_fonts(cx);
        gpui_component::init(cx);
        theme::sync_gpui_component_theme(cx);
        let zoom = theme::zoom();
        let (w, h) = selection.size;
        let window_size = size(px(w * zoom), px(h * zoom));
        let display = selection.display.as_ref().and_then(|prefix| {
            cx.displays().into_iter().find(|display| {
                display
                    .uuid()
                    .is_ok_and(|uuid| uuid.to_string().starts_with(prefix.as_str()))
            })
        });
        if selection.display.is_some() && display.is_none() {
            fail("KAGI_GALLERY_DISPLAY matched no display".into());
        }
        let display_id = display.map(|display| display.id());
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                display_id,
                window_size,
                cx,
            ))),
            display_id,
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(format!("Kagi gallery — {}", selection.page.name).into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(18.), px(18.))),
            }),
            focus: false,
            // With `gui-e2e` a capture renders the scene offscreen through
            // Metal, so the window never needs to be on screen at all.
            show: !(OFFSCREEN && selection.out.is_some()),
            is_resizable: true,
            ..Default::default()
        };
        let page = selection.page;
        let state = selection.state;
        let handle = cx
            .open_window(options, |window, cx| {
                let view = cx.new(|_| GalleryView {
                    page,
                    context: GalleryContext {
                        state,
                        lang: i18n::lang(),
                    },
                });
                cx.new(|cx| Root::new(view, window, cx))
            })
            .unwrap_or_else(|error| fail(format!("open window: {error}")));
        if let Some(limit) = selection.exit_after {
            cx.spawn(async move |cx| {
                cx.background_executor().timer(limit).await;
                eprintln!("gallery: exit-after {} ms reached", limit.as_millis());
                std::process::exit(3);
            })
            .detach();
        }
        if let Some(out) = selection.out.clone() {
            cx.spawn(async move |cx| {
                if !OFFSCREEN && std::env::var("KAGI_GALLERY_RAISE").as_deref() == Ok("1") {
                    let _ = handle.update(cx, |_, window, _| order_front(window));
                }
                // Settling time for fonts, SVG icons and layout.
                cx.background_executor()
                    .timer(Duration::from_millis(900))
                    .await;
                let any: gpui::AnyWindowHandle = handle.into();
                let result = cx
                    .update_window(any, |_, window, cx| capture(window, cx, &out))
                    .unwrap_or_else(|error| Err(format!("window gone: {error}")));
                match result {
                    Ok(()) => {
                        println!("gallery: wrote {}", out.display());
                        std::process::exit(0);
                    }
                    Err(error) => fail(error),
                }
            })
            .detach();
        }
    });
}

/// Built with `--features gui-e2e`, captures are offscreen Metal renders
/// (`Window::render_to_image`, the Tier A path): independent of Spaces,
/// occlusion and the user's displays. Without it, a capture is a
/// `screencapture` of the visible window.
const OFFSCREEN: bool = cfg!(feature = "gui-e2e");

#[cfg(feature = "gui-e2e")]
fn capture(window: &mut Window, cx: &mut App, out: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    window.refresh();
    window.draw(cx).clear();
    let image = window.render_to_image().map_err(|e| e.to_string())?;
    image.save(out).map_err(|e| e.to_string())
}

#[cfg(all(not(feature = "gui-e2e"), target_os = "macos"))]
fn capture(window: &mut Window, _: &mut App, out: &std::path::Path) -> Result<(), String> {
    screen_capture(window, out)
}

#[cfg(all(not(feature = "gui-e2e"), not(target_os = "macos")))]
fn capture(_: &mut Window, _: &mut App, _: &std::path::Path) -> Result<(), String> {
    Err("KAGI_GALLERY_OUT needs macOS or a `--features gui-e2e` build".into())
}

/// `KAGI_GALLERY_RAISE=1`: put the window above other apps' windows on its
/// display for the capture, without activating Kagi or taking key focus
/// (`orderFrontRegardless`). A window behind another app's full-size window
/// is occluded and never paints its first frame.
#[cfg(target_os = "macos")]
fn order_front(window: &Window) {
    use objc2::{msg_send, runtime::AnyObject};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: GPUI's AppKit handle is a live NSView on the main thread.
    unsafe {
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        let native: *mut AnyObject = msg_send![view, window];
        if !native.is_null() {
            let _: () = msg_send![native, orderFrontRegardless];
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn order_front(_: &Window) {}

#[cfg(all(not(feature = "gui-e2e"), target_os = "macos"))]
fn screen_capture(window: &Window, out: &std::path::Path) -> Result<(), String> {
    use objc2::{msg_send, runtime::AnyObject};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("not an AppKit window".into());
    };
    // SAFETY: GPUI's AppKit handle is a live NSView on the main thread.
    let (number, occlusion) = unsafe {
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        let native: *mut AnyObject = msg_send![view, window];
        if native.is_null() {
            return Err("no NSWindow".into());
        }
        let number: isize = msg_send![native, windowNumber];
        let occlusion: usize = msg_send![native, occlusionState];
        (number, occlusion)
    };
    // NSWindowOcclusionStateVisible: a window on another Space or display
    // keeps its last frame, so its capture would be stale (#1799).
    if occlusion & (1 << 1) == 0 {
        return Err(format!(
            "window {number} is not visible (occlusionState {occlusion}); open it on a visible \
             display with KAGI_GALLERY_DISPLAY"
        ));
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let status = std::process::Command::new("/usr/sbin/screencapture")
        .args(["-x", "-o", &format!("-l{number}")])
        .arg(out)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("screencapture exited with {status}"))
    }
}
