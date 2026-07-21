//! Startup registration for fonts embedded in the Kagi binary.

use gpui::App;

use super::CJK_FONT;

pub(super) fn load_bundled_fonts(cx: &mut App) {
    // Keep registration synchronous and before window creation so async views
    // inherit exactly the same available families as the initial render.
    if let Err(e) = cx.text_system().add_fonts(vec![
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/Inter-Regular.ttf")),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/Inter-Bold.ttf")),
        std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/fonts/JetBrainsMono-Regular.ttf"
        )),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf")),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/NotoSansJP-Variable.ttf")),
    ]) {
        klog!("fonts: add_fonts failed (UI may fall back): {e}");
        return;
    }

    // Preserve the existing contract line and append fallback diagnostics.
    klog!("fonts: loaded Inter + JetBrains Mono");
    let cjk_ready = cx
        .text_system()
        .all_font_names()
        .iter()
        .any(|name| name == CJK_FONT);
    if cjk_ready {
        klog!("fonts: fallback {CJK_FONT}");
    } else {
        klog!("fonts: fallback missing {CJK_FONT}");
    }
}
