//! kagi-web — browser (WASM) harness for UI e2e tests.
//!
//! Mounts production portable views and focused pure-domain layout stories with
//! gpui_web so Playwright drives their real input handlers without a native
//! display server. Native effects are supplied only by explicit fixture adapters.

use gpui::{div, prelude::*, px, rems, rgb, SharedString, Window};
#[cfg(target_family = "wasm")]
mod assets;
#[cfg(target_family = "wasm")]
mod settings_fixture;

/// Catalog root: one story per known layout-regression class.
/// Add a story when a UI bug is fixed so the e2e suite guards it.
pub struct Catalog;

impl gpui::Render for Catalog {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .id("catalog-root")
            .size_full()
            .flex()
            .flex_row()
            .bg(rgb(0x15161c))
            .text_color(rgb(0xe6e6ef))
            .font_family("Inter")
            .child(
                // Filler column: the story panel takes 40% of the window, so
                // resizing the browser viewport changes the wrap width — the
                // same stimulus as dragging kagi's inspector divider.
                div().flex_1().min_w(px(0.)),
            )
            .child(inspector_story())
    }
}

/// Story 1 — "inspector-wrap": replica of the inspector header + message
/// layout that jittered on zed-main gpui (fixed 2-line title box, reflowed
/// body in an isolated scroll container). If the fix regresses, the body's
/// Y position oscillates while the viewport width changes and the e2e
/// resize-stability spec fails.
fn inspector_story() -> impl IntoElement {
    let subject = "chore(deps): migrate gpui 0.2.2 \u{2192} zed main git deps (gpui_web-era stack)";
    let body = "\
chore(deps): migrate gpui 0.2.2 \u{2192} zed main git deps (gpui_web-era stack)\n\
\n\
crates.io gpui froze at 0.2.2 (2025-10) while upstream split into\n\
gpui/gpui_platform/gpui_web/gpui_wgpu and kept moving. Switch to rev-less\n\
zed git deps (pin lives in Cargo.lock; gpui-component pinned by rev, which\n\
tracks zed the same way).\n\
\n\
Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>";
    let message = SharedString::from(kagi_domain::message::reflow_message(body));

    let title = div()
        .id("story-inspector-title")
        .font_weight(gpui::FontWeight::MEDIUM)
        .mb_1()
        .whitespace_normal()
        .line_height(rems(1.3))
        .h(rems(2.6)) // fixed 2-line box — see src/ui/inspector.rs
        .overflow_hidden()
        .child(SharedString::from(subject));

    let message_scroll = div()
        .id("story-inspector-message")
        .size_full()
        .min_h(px(0.))
        .overflow_y_scroll()
        .child(
            div()
                .w_full()
                .min_w(px(0.))
                .text_sm()
                .whitespace_normal()
                .child(message),
        );

    div()
        .id("story-inspector")
        .w(gpui::relative(0.4))
        .h_full()
        .flex()
        .flex_col()
        .border_l_1()
        .border_color(rgb(0x33344a))
        .p_3()
        .child(title)
        .child(
            div()
                .flex()
                .flex_col()
                .min_h(px(0.))
                .flex_basis(gpui::relative(0.6))
                .flex_shrink(1.)
                .overflow_hidden()
                .child(message_scroll),
        )
        .child(div().flex_1().min_h(px(0.)))
}

#[cfg(target_family = "wasm")]
mod web {
    use super::*;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub fn run(story: Option<String>) -> Result<(), JsValue> {
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);

        gpui_platform::web_init();
        let app = gpui_platform::single_threaded_web().with_assets(assets::Assets);
        // Keep the app alive for the page's lifetime (same workaround as
        // upstream story-web): leak the Rc so drop never runs.
        let app = {
            struct WasmApplication(std::rc::Rc<gpui::AppCell>);
            let wasm_app =
                unsafe { std::mem::transmute::<gpui::Application, WasmApplication>(app) };
            std::mem::forget(wasm_app.0.clone());
            unsafe { std::mem::transmute::<WasmApplication, gpui::Application>(wasm_app) }
        };

        app.run(move |cx: &mut gpui::App| {
            // Browsers have no system fonts we can rely on — embed Inter
            // (same OFL files the native app bundles).
            if let Err(e) = cx.text_system().add_fonts(vec![
                std::borrow::Cow::Borrowed(
                    include_bytes!("../../../assets/fonts/Inter-Regular.ttf").as_slice(),
                ),
                std::borrow::Cow::Borrowed(
                    include_bytes!("../../../assets/fonts/Inter-Bold.ttf").as_slice(),
                ),
                std::borrow::Cow::Borrowed(
                    include_bytes!("../../../assets/fonts/NotoSansJP-Regular.ttf").as_slice(),
                ),
                std::borrow::Cow::Borrowed(
                    include_bytes!("../../../assets/fonts/NotoSansJP-Bold.ttf").as_slice(),
                ),
            ]) {
                log::warn!("add_fonts failed: {e}");
            }
            gpui_component::init(cx);
            kagi_ui_core::keyboard_nav::bind_keys(cx);
            if story.as_deref() == Some("settings") {
                cx.open_window(gpui::WindowOptions::default(), |window, cx| {
                    let view = cx.new(|cx| settings_fixture::SettingsFixture::new(window, cx));
                    cx.new(|cx| gpui_component::Root::new(view, window, cx))
                })
                .expect("open Settings verification window");
            } else {
                cx.open_window(gpui::WindowOptions::default(), |_, cx| cx.new(|_| Catalog))
                    .expect("open window");
            }
        });
        Ok(())
    }

    #[wasm_bindgen]
    pub fn settings_observed() -> String {
        settings_fixture::observed()
    }

    #[wasm_bindgen]
    pub fn settings_command(command: &str, value: Option<String>) -> Result<(), JsValue> {
        settings_fixture::command(command, value.as_deref())
            .map_err(|error| JsValue::from_str(&error))
    }
}
